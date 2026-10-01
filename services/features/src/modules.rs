//! A .xmd feature module as a provider: it consumes the same semantic records
//! as queries and returns data only, and this adapter validates every
//! position and edit it proposes.
use crate::{
    commands::{
        self, Action, ActionProvider, Capabilities, CommandId, Control, NOT_MINE, Prepared,
        Proposal, SOURCE_CHANGED,
    },
    inlays::{InlayContext, InlaySink},
};
use chrono::{DateTime, FixedOffset};
use lang::document::{Document, LineIndex};
use lang::eval::engine::{Engine, Value};
use lang::eval::modules::{Hook, Module, ModuleKind, from_json, json};
use lang::eval::{EvalError, ToValue, record};
use lsp_types::{Diagnostic, DiagnosticSeverity, Hover, HoverContents, Position, Range, TextEdit};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

record! {
    /// What the host can do with an action the module proposes.
    struct CapabilitiesInput {
        refresh: bool,
        views: bool,
    }
}

/// Everything a feature module's hook is handed. Built once per call, so the
/// context a module sees is one typed value rather than a record assembled
/// field by field at each call site.
#[derive(Clone, Default)]
pub(crate) struct HookInput {
    /// What every hook shares: `today`, `midnight`, `document`, `module`.
    shared: BTreeMap<String, Value>,
    range: Option<Range>,
    capabilities: Option<Capabilities>,
    position: Option<Position>,
}
impl ToValue for HookInput {
    fn to_value(&self) -> Value {
        let mut fields = self.shared.clone();
        // The optional ones are absent, not null, when the call has no use
        // for them: a hook tells them apart with `has`.
        if let Some(range) = self.range {
            fields.insert("range".into(), from_json(&serde_json::json!(range)));
        }
        if let Some(capabilities) = self.capabilities {
            fields.insert(
                "capabilities".into(),
                CapabilitiesInput {
                    refresh: capabilities.refresh,
                    views: capabilities.views,
                }
                .to_value(),
            );
        }
        if let Some(position) = self.position {
            fields.insert("position".into(), from_json(&serde_json::json!(position)));
        }
        Value::record(fields)
    }
}

/// One inlay a module proposed, with its position already resolved against the
/// document and validated.
struct InlayHint {
    position: Position,
    label: String,
    tooltip: String,
}
impl TryFrom<(&Bounds<'_>, Value)> for InlayHint {
    type Error = String;
    fn try_from((bounds, value): (&Bounds<'_>, Value)) -> Result<Self, String> {
        let Value::Record(fields) = value else {
            return Err("Each inlay must be a record".into());
        };
        let position = if let Some(at) = fields.get("at") {
            serde_json::from_value::<Position>(json(at)?).map_err(|e| e.to_string())?
        } else {
            let line = fields.get("line").ok_or("Inlay needs at or line")?;
            bounds.document.line_end(bounds.line(line, "Inlay")?)
        };
        bounds.validate(Range::new(position, position))?;
        let Some(Value::Text(label)) = fields.get("label") else {
            return Err("Inlay label must be text".into());
        };
        let tooltip = match fields.get("tooltip") {
            None => String::new(),
            Some(Value::Text(s)) => s.clone(),
            _ => return Err("Inlay tooltip must be text".into()),
        };
        Ok(Self {
            position,
            label: label.clone(),
            tooltip,
        })
    }
}

/// Decode one action a module proposed for a line, before its capabilities
/// and source are checked.
fn proposal(bounds: &Bounds<'_>, value: Value) -> Result<Control, String> {
    let Value::Record(fields) = value else {
        return Err("Each action must be a record".into());
    };
    let line = bounds.line(fields.get("line").ok_or("Action needs a line")?, "Action")?;
    let Some(Value::Text(title)) = fields.get("title") else {
        return Err("Action title must be text".into());
    };
    let action: Action =
        serde_json::from_value(json(fields.get("action").ok_or("Missing action")?)?)
            .map_err(|e| e.to_string())?;
    let disabled = match fields.get("disabled") {
        None | Some(Value::Null) => None,
        Some(Value::Text(reason)) => Some(reason.clone()),
        Some(_) => return Err("Action disabled must be text or null".into()),
    };
    Ok(Control {
        proposal: Proposal {
            line,
            title: title.clone(),
            action,
        },
        disabled,
    })
}

/// One hover a module proposed, before its range is validated.
struct HoverItem {
    range: Range,
    contents: String,
    /// Shown only where the editor's own hover finds nothing more specific.
    fallback: bool,
}
impl TryFrom<serde_json::Value> for HoverItem {
    type Error = String;
    fn try_from(item: serde_json::Value) -> Result<Self, String> {
        let range: Range =
            serde_json::from_value(item["range"].clone()).map_err(|e| e.to_string())?;
        let contents = item["contents"]
            .as_str()
            .ok_or("Hover contents must be text")?;
        let fallback = match &item["fallback"] {
            serde_json::Value::Null => false,
            serde_json::Value::Bool(fallback) => *fallback,
            _ => return Err("Hover fallback must be true or false".into()),
        };
        Ok(Self {
            range,
            contents: contents.to_owned(),
            fallback,
        })
    }
}

/// What `module`'s hooks are handed for the note at `path`: the context
/// every hook shares, read from the request's shared records, with `engine`
/// marked by whatever clock reading they took.
pub(crate) fn input(
    module: &Module,
    request: &crate::Request<'_>,
    engine: &mut Engine<'_>,
    path: &Path,
) -> Result<HookInput, String> {
    Ok(HookInput {
        shared: records::feature_context(&request.records, engine, module, path, false)?,
        ..HookInput::default()
    })
}

/// The note a hook's items are checked against, measured once per call
/// rather than once per item.
struct Bounds<'a> {
    document: &'a Document,
    index: LineIndex<'a>,
    lines: usize,
}
impl<'a> Bounds<'a> {
    fn new(document: &'a Document) -> Self {
        Self {
            document,
            index: LineIndex::new(document.text()),
            lines: document.text().lines().count(),
        }
    }
    fn of(request: &crate::Request<'a>, path: &Path) -> Self {
        Self::new(&request.workspace().documents()[path])
    }
    /// The row a `what` names with `line`, while the note has it.
    fn line(&self, line: &Value, what: &str) -> Result<usize, String> {
        let line = json(line)?
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or(format!("{what} line must be a nonnegative integer"))?;
        if line >= self.lines {
            return Err(format!("{what} line is outside the document"));
        }
        Ok(line)
    }
    /// Whether `range` lies within the note on UTF-16 boundaries: what
    /// applying an edit over it would check.
    fn validate(&self, range: Range) -> Result<(), String> {
        if range.start > range.end {
            return Err("Reversed edit range".into());
        }
        self.index.offset(range.start)?;
        self.index.offset(range.end)?;
        Ok(())
    }
}

/// What feature modules' hooks answered, for a later call with the same
/// input at a clock the answer still holds for. Module code is pure but for
/// the clock, so an answer holds for as long as the clock reads the same to
/// it: all day (the input carries the date), or — when the call read the clock
/// more finely, as [`lang::eval::reads_clock`] reports — only at that
/// instant. The module registry is the workspace's, so an owner keeps these
/// only for one workspace revision.
#[derive(Default)]
pub(crate) struct Answers {
    answers: Mutex<Vec<Answer>>,
}
struct Answer {
    module: String,
    hook: Hook,
    input: Value,
    now: DateTime<FixedOffset>,
    exact: bool,
    result: lang::eval::EvalResult<Value>,
}
impl Answer {
    fn holds_at(&self, now: DateTime<FixedOffset>) -> bool {
        self.now.offset() == now.offset()
            && if self.exact {
                self.now == now
            } else {
                self.now.date_naive() == now.date_naive()
            }
    }
}
/// How many answers a request's owner keeps: enough for every hook of every
/// feature module over the notes open at once.
const ANSWERS: usize = 256;
impl Answers {
    /// `module`'s answer to `hook` with `input` at `now`.
    fn call(
        &self,
        module: &Module,
        hook: Hook,
        input: &HookInput,
        now: DateTime<FixedOffset>,
    ) -> lang::eval::EvalResult<Value> {
        let input = input.to_value();
        let lock = || {
            self.answers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
        };
        if let Some(answer) = lock().iter().find(|answer| {
            answer.module == module.id
                && answer.hook == hook
                && answer.holds_at(now)
                && answer.input == input
        }) {
            return answer.result.clone();
        }
        let (result, exact) =
            lang::eval::reads_clock(|| module.call(hook, vec![input.clone()], now));
        let mut answers = lock();
        answers.retain(|answer| answer.holds_at(now));
        if answers.len() >= ANSWERS {
            answers.remove(0);
        }
        answers.push(Answer {
            module: module.id.clone(),
            hook,
            input,
            now,
            exact,
            result: result.clone(),
        });
        result
    }
}

#[cfg(test)]
impl Answers {
    /// The clock each kept answer was given at.
    pub(crate) fn clocks(&self) -> Vec<DateTime<FixedOffset>> {
        let answers = self.answers.lock().unwrap();
        answers.iter().map(|answer| answer.now).collect()
    }
}

/// Call one hook with `input` and decode each item of the list it returns.
fn hook_items<T>(
    request: &crate::Request<'_>,
    module: &Module,
    hook: Hook,
    input: &HookInput,
    decode: impl FnMut(Value) -> Result<T, String>,
) -> Result<Vec<T>, String> {
    let answer = request.answers.call(module, hook, input, request.now())?;
    let Value::List(items) = answer else {
        return Err(format!("{hook} must return a list"));
    };
    Arc::unwrap_or_clone(items)
        .into_inner()
        .into_iter()
        .map(decode)
        .collect()
}
/// Call one data-only hook, when the module defines it, with `position` as
/// `ctx.position`, and decode its items once every one of them has converted
/// to JSON.
fn json_items<T>(
    module: &Module,
    request: &crate::Request<'_>,
    path: &Path,
    hook: Hook,
    position: Option<Position>,
    decode: impl FnMut(serde_json::Value) -> Result<T, String>,
) -> Result<Vec<T>, String> {
    if !module.has(hook) {
        return Ok(vec![]);
    }
    let input = HookInput {
        position,
        ..input(module, request, &mut request.engine(), path)?
    };
    hook_items(request, module, hook, &input, |item| Ok(json(&item)?))?
        .into_iter()
        .map(decode)
        .collect()
}

// Each hook a feature module defines answers for one kind.

pub(crate) fn inlays(module: &Module, context: &mut InlayContext<'_, '_>, output: &mut InlaySink) {
    if !module.enabled || module.kind != ModuleKind::Feature || !module.has(Hook::Collect) {
        return;
    }
    let document = context.document;
    let result = input(module, context.request, context.engine, context.path).and_then(|input| {
        let input = HookInput {
            range: Some(context.range),
            ..input
        };
        let request = context.request;
        if module.has(Hook::TimeDependent) {
            let answer = request
                .answers
                .call(module, Hook::TimeDependent, &input, request.now());
            if answer? == Value::Bool(true) {
                context.engine.mark_time_dependent(true);
            }
        } else if module.live {
            context.engine.mark_time_dependent(true);
        }
        // Whether labels move with the clock is decided above; the labels
        // themselves never change that.
        if !context.labels {
            return Ok(vec![]);
        }
        let bounds = Bounds::new(document);
        hook_items(request, module, Hook::Collect, &input, |hint| {
            InlayHint::try_from((&bounds, hint))
        })
    });
    match result {
        Ok(hints) => {
            for hint in hints {
                output.push(hint.position, hint.label, hint.tooltip);
            }
        }
        Err(error) => output.push(
            document.line_end(0),
            format!("module error · {}", module.id),
            error,
        ),
    }
}
/// Actions are proposals, each for one line of the note, gathered in one call.
/// Each is checked with `check` (its owner's preparation) before it is
/// exposed, and a line's controls stand or fall together; the host repeats
/// validation against the execution snapshot. A control the module says is
/// disabled is kept with its reason, unchecked. Gives each line's controls,
/// or why that line's are withheld; `Err` when the hook itself fails.
pub(crate) fn controls(
    module: &Module,
    request: &crate::Request<'_>,
    path: &Path,
    capabilities: Capabilities,
    check: &dyn Fn(&Action) -> Result<(), String>,
) -> Result<BTreeMap<usize, Result<Vec<Control>, String>>, String> {
    if !module.has(Hook::Actions) {
        return Ok(BTreeMap::new());
    }
    let bounds = Bounds::of(request, path);
    let proposals = input(module, request, &mut request.engine(), path).and_then(|input| {
        let input = HookInput {
            capabilities: Some(capabilities),
            ..input
        };
        hook_items(request, module, Hook::Actions, &input, |item| {
            proposal(&bounds, item)
        })
    })?;
    let mut lines: BTreeMap<usize, Result<Vec<Control>, String>> = BTreeMap::new();
    for control in proposals {
        let entry = lines
            .entry(control.proposal.line)
            .or_insert_with(|| Ok(vec![]));
        let Ok(kept) = entry else {
            continue;
        };
        if !capabilities.supports(&control.proposal.action) {
            continue;
        }
        if control.disabled.is_some() {
            kept.push(control);
            continue;
        }
        match check(&control.proposal.action) {
            Ok(()) => kept.push(control),
            Err(error) => *entry = Err(error),
        }
    }
    Ok(lines)
}
/// Why a module's controls are missing, from what [`controls`] gave with a
/// native host's capabilities: a failing `actions` hook, or a line whose
/// controls did not prepare. A lens has no disabled state, so the reason is
/// a warning where the controls would be.
pub(crate) fn control_problems(
    module: &Module,
    controls: Result<BTreeMap<usize, Result<Vec<Control>, String>>, String>,
) -> Vec<Diagnostic> {
    let warning =
        |range, message| analysis::module_problem(DiagnosticSeverity::WARNING, range, message);
    match controls {
        // The error already names the module and hook.
        Err(error) => vec![warning(Range::default(), error)],
        Ok(lines) => lines
            .into_iter()
            .filter_map(|(line, commands)| {
                let error = commands.err()?;
                let at = Position::new(line as u32, 0);
                Some(warning(
                    Range::new(at, at),
                    format!("{} controls on this line are withheld: {error}", module.id),
                ))
            })
            .collect(),
    }
}
/// A hook that fails is itself a diagnostic, naming the module.
pub(crate) fn diagnostics(
    module: &Module,
    request: &crate::Request<'_>,
    path: &Path,
) -> Vec<Diagnostic> {
    let mut bounds = None;
    json_items(module, request, path, Hook::Diagnostics, None, |item| {
        let mut diagnostic: Diagnostic = serde_json::from_value(item).map_err(|e| e.to_string())?;
        bounds
            .get_or_insert_with(|| Bounds::of(request, path))
            .validate(diagnostic.range)?;
        diagnostic.source.get_or_insert("xmd".into());
        Ok(diagnostic)
    })
    .unwrap_or_else(|error| {
        let message = format!("{}: {error}", module.id);
        vec![analysis::module_problem(
            DiagnosticSeverity::ERROR,
            Range::default(),
            message,
        )]
    })
}
/// The module's hover at `at`, handed in as `ctx.position`: one it words
/// first, or with `fallback`, one it words for where the editor's own hover
/// finds nothing more specific.
pub(crate) fn hover(
    module: &Module,
    request: &crate::Request<'_>,
    path: &Path,
    at: Position,
    fallback: bool,
) -> Option<Hover> {
    let mut bounds = None;
    json_items(module, request, path, Hook::Hovers, Some(at), |item| {
        let hover = HoverItem::try_from(item)?;
        bounds
            .get_or_insert_with(|| Bounds::of(request, path))
            .validate(hover.range)?;
        Ok(hover)
    })
    .ok()?
    .into_iter()
    .find(|hover| hover.fallback == fallback && at >= hover.range.start && at <= hover.range.end)
    .map(|hover| Hover {
        range: Some(hover.range),
        contents: HoverContents::Markup(analysis::markup(hover.contents)),
    })
}
/// The outline entries a module adds, each checked against the note.
pub(crate) fn symbols(
    module: &Module,
    request: &crate::Request<'_>,
    path: &Path,
) -> Vec<analysis::Outlined> {
    let bounds = Bounds::of(request, path);
    json_items(module, request, path, Hook::Symbols, None, |item| {
        let line = |key: &str| item[key].as_u64().and_then(|n| usize::try_from(n).ok());
        let row = line("line")
            .filter(|row| *row < bounds.lines)
            .ok_or("A symbol needs a line within the note")?;
        let selection: Range =
            serde_json::from_value(item["selection"].clone()).map_err(|e| e.to_string())?;
        bounds.validate(selection)?;
        let text = |key: &str| item[key].as_str().map(str::to_owned);
        Ok(analysis::Outlined {
            name: text("name").ok_or("A symbol's name must be text")?,
            detail: text("detail").unwrap_or_default(),
            kind: match text("kind") {
                None => lsp_types::SymbolKind::NAMESPACE,
                Some(kind) => {
                    lsp_kind(SYMBOL_KINDS, &kind).ok_or(format!("Unknown symbol kind '{kind}'"))?
                }
            },
            line: row,
            end_line: line("end_line").unwrap_or(row + 1),
            selection,
        })
    })
    .unwrap_or_default()
}
/// An LSP enum by its snake-case name, among `names` listed in protocol
/// order from 1.
fn lsp_kind<T: serde::de::DeserializeOwned>(names: &str, name: &str) -> Option<T> {
    let index = names.split_whitespace().position(|known| known == name)?;
    serde_json::from_value((index + 1).into()).ok()
}
const SYMBOL_KINDS: &str = "file module namespace package class method property field \
    constructor enum interface function variable constant string number boolean array object \
    key null enum_member struct event operator type_parameter";
const COMPLETION_KINDS: &str = "text method function constructor field variable class \
    interface module property unit value enum keyword snippet color file reference folder \
    enum_member constant struct event operator type_parameter";
/// What a module offers at `position`, each item replacing `replacement`:
/// `None` when it leaves the position to the editor, or fails.
pub(crate) fn completions(
    module: &Module,
    request: &crate::Request<'_>,
    path: &Path,
    position: Position,
    replacement: Range,
) -> Option<Vec<lsp_types::CompletionItem>> {
    if !module.has(Hook::Completions) {
        return None;
    }
    let input = HookInput {
        position: Some(position),
        ..input(module, request, &mut request.engine(), path).ok()?
    };
    let answer = request
        .answers
        .call(module, Hook::Completions, &input, request.now())
        .ok()?;
    let Value::List(items) = answer else {
        return None;
    };
    items
        .iter()
        .map(|item| {
            let item = json(item).ok()?;
            let text = |key: &str| item[key].as_str().map(str::to_owned);
            let label = text("label")?;
            let insert = text("insert").unwrap_or_else(|| label.clone());
            Some(lsp_types::CompletionItem {
                label,
                detail: text("detail"),
                kind: match text("kind") {
                    None => None,
                    Some(kind) => Some(lsp_kind(COMPLETION_KINDS, &kind)?),
                },
                text_edit: crate::completion::replace(replacement, insert),
                ..Default::default()
            })
        })
        .collect()
}
/// The edits formatting makes; `at` is where a `|` was just typed when the
/// note is formatted as it is typed.
pub(crate) fn edits(
    module: &Module,
    request: &crate::Request<'_>,
    path: &Path,
    at: Option<Position>,
) -> Result<Vec<TextEdit>, String> {
    json_items(module, request, path, Hook::Format, at, |item| {
        serde_json::from_value(item).map_err(|e| e.to_string())
    })
}

/// Feature modules own the invocation kind: a control a module proposed that
/// runs its reducer when executed. The reducer answers with the concrete
/// action the invocation stands for, which its own owner then prepares.
pub(crate) struct Invocations;
impl ActionProvider for Invocations {
    fn kinds(&self) -> &'static [CommandId] {
        &[CommandId::Invoke, CommandId::Row]
    }
    fn prepare(
        &self,
        request: &crate::Request<'_>,
        action: &Action,
        capabilities: Capabilities,
    ) -> Result<Prepared, String> {
        let (Action::Invoke { event, .. } | Action::Row { event, .. }) = action else {
            return Err(NOT_MINE.into());
        };
        let (path, module) = invocation(request, action)?;
        reduce(request, &path, module, event, capabilities).map(Prepared::Reduced)
    }
    /// A proposed invocation is checked without running its reducer.
    fn check(
        &self,
        request: &crate::Request<'_>,
        action: &Action,
        _: Capabilities,
    ) -> Result<(), String> {
        invocation(request, action).map(drop)
    }
}
/// The module an invocation still reaches, with the document it acts on:
/// the whole note must read as it did for an invocation, only its row for a
/// row action, and an invocation's module must be at the same revision.
fn invocation<'a>(
    request: &crate::Request<'a>,
    action: &Action,
) -> Result<(PathBuf, &'a Module), String> {
    let (path, module, revision) = match action {
        Action::Invoke {
            document,
            expected,
            module,
            revision,
            ..
        } => {
            let (path, doc) = commands::document(request, document)?;
            if doc.text() != *expected {
                return Err(SOURCE_CHANGED.into());
            }
            (path, module, Some(revision))
        }
        Action::Row {
            document,
            row,
            expected,
            module,
            ..
        } => {
            let (path, _) = commands::row_document(request, document, *row, expected)?;
            (path, module, None)
        }
        _ => return Err("Expected a module invocation".into()),
    };
    let module = request
        .workspace()
        .modules()
        .get(module)
        .ok_or("Module is no longer available")?;
    if revision.is_some_and(|revision| module.revision() != *revision) {
        return Err("Module changed; request fresh controls".into());
    }
    if !module.has(Hook::Reduce) {
        return Err("Module has no reducer".into());
    }
    Ok((path, module))
}
fn reduce(
    request: &crate::Request<'_>,
    path: &Path,
    module: &Module,
    event: &serde_json::Value,
    capabilities: Capabilities,
) -> Result<Action, String> {
    let context = HookInput {
        capabilities: Some(capabilities),
        ..input(module, request, &mut request.engine(), path)?
    };
    let result = module
        .call(
            Hook::Reduce,
            vec![context.to_value(), from_json(event)],
            request.now(),
        )
        .map_err(refusal)?;
    let action: Action = serde_json::from_value(json(&result)?).map_err(|e| e.to_string())?;
    if matches!(action, Action::Invoke { .. } | Action::Row { .. }) {
        return Err("A reducer must return a concrete action".into());
    }
    Ok(action)
}
/// Why a reducer failed. One that refuses with `error(reason)` has said why
/// the control cannot run, which the person reads as written; any other
/// failure names the module.
fn refusal(error: EvalError) -> String {
    match &error {
        EvalError::Module { source, .. }
            if matches!(**source, EvalError::Custom(_) | EvalError::Pending(_)) =>
        {
            source.to_string()
        }
        _ => error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lsp_types::{CompletionItemKind, SymbolKind};

    /// The kind names are the protocol's, in its order.
    #[test]
    fn kinds_are_named_in_protocol_order() {
        let symbol = |name| lsp_kind::<SymbolKind>(SYMBOL_KINDS, name);
        assert_eq!(symbol("file"), Some(SymbolKind::FILE));
        assert_eq!(symbol("enum_member"), Some(SymbolKind::ENUM_MEMBER));
        assert_eq!(symbol("type_parameter"), Some(SymbolKind::TYPE_PARAMETER));
        assert_eq!(symbol("File"), None);
        let completion = |name| lsp_kind::<CompletionItemKind>(COMPLETION_KINDS, name);
        assert_eq!(completion("text"), Some(CompletionItemKind::TEXT));
        assert_eq!(
            completion("type_parameter"),
            Some(CompletionItemKind::TYPE_PARAMETER)
        );
    }
}
