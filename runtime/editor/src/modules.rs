//! A .xmd feature module as a provider: it consumes the same semantic records
//! as queries and returns data only, and this adapter validates every
//! position and edit it proposes.
use crate::{
    commands::{
        self, Action, ActionProvider, Capabilities, CommandId, NOT_MINE, Prepared, Proposal,
        SOURCE_CHANGED,
    },
    inlays::{InlayContext, InlaySink},
};
use catalog::View;
use chrono::{DateTime, FixedOffset, NaiveDate};
use lang::eval::engine::{Engine, Value};
use lang::eval::modules::{Collection, Hook, Module, ModuleKind, from_json, json};
use lang::eval::{ToValue, record};
use lang::model::{Document, LineIndex};
use lsp_types::{
    Command, Diagnostic, DiagnosticSeverity, Hover, HoverContents, MarkupContent, MarkupKind,
    NumberOrString, Position, Range, TextEdit,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

record! {
    /// The note a hook is looking at, with the record collections it declared
    /// under their own names.
    #[derive(Clone)]
    struct DocumentInput {
        ..collections: BTreeMap<String, Value>,
        path: String,
        uri: String,
        text: String,
        lines: Vec<String>,
    }
}

record! {
    /// Which module is asking, and at what revision.
    #[derive(Clone)]
    struct ModuleInput {
        id: String,
        revision: String,
    }
}

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
#[derive(Clone)]
pub(crate) struct HookInput {
    today: NaiveDate,
    document: DocumentInput,
    module: ModuleInput,
    range: Option<Range>,
    capabilities: Option<Capabilities>,
}
impl HookInput {
    const TODAY: &'static str = "today";
    const DOCUMENT: &'static str = "document";
    const MODULE: &'static str = "module";
    const RANGE: &'static str = "range";
    const CAPABILITIES: &'static str = "capabilities";
    fn with_range(mut self, range: Range) -> Self {
        self.range = Some(range);
        self
    }
    fn with_capabilities(mut self, capabilities: Capabilities) -> Self {
        self.capabilities = Some(capabilities);
        self
    }
}
impl ToValue for HookInput {
    fn to_value(&self) -> Value {
        let mut fields = BTreeMap::from([
            (Self::TODAY.into(), self.today.to_value()),
            (Self::DOCUMENT.into(), self.document.to_value()),
            (Self::MODULE.into(), self.module.to_value()),
        ]);
        // The optional two are absent, not null, when the call has no use
        // for them: a hook tells them apart with `has`.
        if let Some(range) = self.range {
            fields.insert(Self::RANGE.into(), from_json(&serde_json::json!(range)));
        }
        if let Some(capabilities) = self.capabilities {
            fields.insert(
                Self::CAPABILITIES.into(),
                CapabilitiesInput {
                    refresh: capabilities.refresh,
                    views: capabilities.views,
                }
                .to_value(),
            );
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
            let line = json(line)?
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or("Inlay line must be a nonnegative integer")?;
            if line >= bounds.lines {
                return Err("Inlay line is outside the document".into());
            }
            bounds.document.line_end(line)
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
fn proposal(bounds: &Bounds<'_>, value: Value) -> Result<Proposal, String> {
    {
        let Value::Record(fields) = value else {
            return Err("Each action must be a record".into());
        };
        let line = json(fields.get("line").ok_or("Action needs a line")?)?
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or("Action line must be a nonnegative integer")?;
        if line >= bounds.lines {
            return Err("Action line is outside the document".into());
        }
        let Some(Value::Text(title)) = fields.get("title") else {
            return Err("Action title must be text".into());
        };
        let action: Action =
            serde_json::from_value(json(fields.get("action").ok_or("Missing action")?)?)
                .map_err(|e| e.to_string())?;
        Ok(Proposal {
            line,
            title: title.clone(),
            action,
        })
    }
}

/// One hover a module proposed, before its range is validated.
struct HoverItem {
    range: Range,
    contents: String,
}
impl TryFrom<serde_json::Value> for HoverItem {
    type Error = String;
    fn try_from(item: serde_json::Value) -> Result<Self, String> {
        let range: Range =
            serde_json::from_value(item["range"].clone()).map_err(|e| e.to_string())?;
        let contents = item["contents"]
            .as_str()
            .ok_or("Hover contents must be text")?;
        Ok(Self {
            range,
            contents: contents.to_owned(),
        })
    }
}

/// What `module`'s hooks are handed for the note at `path`. Each declared
/// collection comes from the request's shared records, narrowed to the fields
/// the module asked for, and `engine` is marked with whatever clock reading
/// they took.
pub(crate) fn input(
    module: &Module,
    request: &crate::Request<'_>,
    engine: &mut Engine<'_>,
    path: &Path,
) -> Result<HookInput, String> {
    let doc = &engine.workspace().documents()[path];
    let mut document = DocumentInput {
        path: path.to_string_lossy().into(),
        uri: lang::common::file_url(path)?.into(),
        text: doc.text.clone(),
        lines: doc.text.lines().map(str::to_owned).collect(),
        collections: BTreeMap::new(),
    };
    for collection in &module.inputs {
        let view = match module.fields.get(collection) {
            Some(fields) => View::Fields(fields),
            None => View::Full,
        };
        let read = |engine: &mut Engine<'_>, view| {
            request
                .records
                .view(engine, Some(path), *collection, view, |request, path| {
                    analysis::collect_native(request, path, false)
                })
        };
        let mut values = read(engine, view)?;
        if *collection == Collection::Recognized {
            // A module sees only its own recognizers' matches. Which module
            // made a match is read off the full records, which a narrowed
            // view may leave out.
            let full = read(engine, View::Full)?;
            values = own(&values, &full, &module.id);
        }
        if *collection == Collection::Values {
            // Preserve the original API's alias; all new fields come from the catalog.
            let mut definitions = values.clone();
            if let Value::List(items) = &mut definitions {
                for item in Arc::make_mut(items) {
                    if let Value::Record(fields) = item {
                        let error = match fields.get("errors") {
                            Some(Value::List(errors)) => {
                                errors.first().cloned().unwrap_or(Value::Null)
                            }
                            _ => Value::Null,
                        };
                        Arc::make_mut(fields).insert("error".into(), error);
                    }
                }
            }
            document
                .collections
                .insert("definitions".into(), definitions);
        }
        document
            .collections
            .insert(<&str>::from(collection).into(), values);
    }
    Ok(HookInput {
        today: engine.today(),
        document,
        module: ModuleInput {
            id: module.id.clone(),
            revision: module.revision(),
        },
        range: None,
        capabilities: None,
    })
}

/// The items of `values` whose record in `full`, the same list unnarrowed,
/// was made by module `id`.
fn own(values: &Value, full: &Value, id: &str) -> Value {
    let (Value::List(values), Value::List(full)) = (values, full) else {
        return values.clone();
    };
    let mine = |record: &Value| {
        matches!(record, Value::Record(fields)
            if matches!(fields.get("module"), Some(Value::Text(module)) if module == id))
    };
    Value::list(
        values
            .iter()
            .zip(full.iter())
            .filter(|(_, record)| mine(record))
            .map(|(value, _)| value.clone())
            .collect(),
    )
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
            index: LineIndex::new(&document.text),
            lines: document.text.lines().count(),
        }
    }
    fn of(request: &crate::Request<'a>, path: &Path) -> Self {
        Self::new(&request.workspace().documents()[path])
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

/// Call one hook with `input` and decode each item of the list it returns.
fn hook_items<T>(
    module: &Module,
    hook: Hook,
    input: &HookInput,
    now: DateTime<FixedOffset>,
    decode: impl FnMut(Value) -> Result<T, String>,
) -> Result<Vec<T>, String> {
    let Value::List(items) = module.call(hook, vec![input.to_value()], now)? else {
        return Err(format!("{hook} must return a list"));
    };
    Arc::unwrap_or_clone(items)
        .into_iter()
        .map(decode)
        .collect()
}
/// Call one data-only hook, when the module defines it, and decode its items
/// once every one of them has converted to JSON.
fn json_items<T>(
    module: &Module,
    request: &crate::Request<'_>,
    path: &Path,
    hook: Hook,
    decode: impl FnMut(serde_json::Value) -> Result<T, String>,
) -> Result<Vec<T>, String> {
    if !module.has(hook) {
        return Ok(vec![]);
    }
    let input = input(module, request, &mut request.engine(), path)?;
    hook_items(module, hook, &input, request.now(), |item| Ok(json(&item)?))?
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
        let input = input.with_range(context.range);
        let now = context.engine.now();
        if module.has(Hook::TimeDependent) {
            if module.call(Hook::TimeDependent, vec![input.to_value()], now)? == Value::Bool(true) {
                context.mark_time_dependent();
            }
        } else if module.live {
            context.mark_time_dependent();
        }
        // Whether labels move with the clock is decided above; the labels
        // themselves never change that.
        if !context.labels {
            return Ok(vec![]);
        }
        let bounds = Bounds::new(document);
        hook_items(module, Hook::Collect, &input, now, |hint| {
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
/// validation against the execution snapshot. Gives each line's controls, or
/// why that line's are withheld; `Err` when the hook itself fails.
pub(crate) fn controls(
    module: &Module,
    request: &crate::Request<'_>,
    path: &Path,
    capabilities: Capabilities,
    check: &dyn Fn(&Action) -> Result<(), String>,
) -> Result<BTreeMap<usize, Result<Vec<Command>, String>>, String> {
    if !module.has(Hook::Actions) {
        return Ok(BTreeMap::new());
    }
    let bounds = Bounds::of(request, path);
    let proposals = input(module, request, &mut request.engine(), path).and_then(|input| {
        let input = input.with_capabilities(capabilities);
        hook_items(module, Hook::Actions, &input, request.now(), |item| {
            proposal(&bounds, item)
        })
    })?;
    let mut lines: BTreeMap<usize, Result<Vec<Command>, String>> = BTreeMap::new();
    for Proposal {
        line,
        title,
        action,
    } in proposals
    {
        let entry = lines.entry(line).or_insert_with(|| Ok(vec![]));
        let Ok(commands) = entry else {
            continue;
        };
        if !capabilities.supports(&action) {
            continue;
        }
        match check(&action) {
            Ok(()) => commands.push(action.command(title)),
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
    controls: Result<BTreeMap<usize, Result<Vec<Command>, String>>, String>,
) -> Vec<Diagnostic> {
    let warning = |range: Range, message: String| Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::WARNING),
        source: Some("xmd".into()),
        code: Some(NumberOrString::String("module".into())),
        message,
        ..Default::default()
    };
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
    json_items(module, request, path, Hook::Diagnostics, |item| {
        let mut diagnostic: Diagnostic = serde_json::from_value(item).map_err(|e| e.to_string())?;
        bounds
            .get_or_insert_with(|| Bounds::of(request, path))
            .validate(diagnostic.range)?;
        diagnostic.source.get_or_insert("xmd".into());
        Ok(diagnostic)
    })
    .unwrap_or_else(|error| {
        vec![Diagnostic {
            range: Range::default(),
            severity: Some(DiagnosticSeverity::ERROR),
            source: Some("xmd".into()),
            code: Some(NumberOrString::String("module".into())),
            message: format!("{}: {error}", module.id),
            ..Default::default()
        }]
    })
}
pub(crate) fn hover(
    module: &Module,
    request: &crate::Request<'_>,
    path: &Path,
    at: Position,
) -> Option<Hover> {
    let mut bounds = None;
    json_items(module, request, path, Hook::Hovers, |item| {
        let hover = HoverItem::try_from(item)?;
        bounds
            .get_or_insert_with(|| Bounds::of(request, path))
            .validate(hover.range)?;
        Ok(hover)
    })
    .ok()?
    .into_iter()
    .find(|hover| at >= hover.range.start && at <= hover.range.end)
    .map(|hover| Hover {
        range: Some(hover.range),
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: hover.contents,
        }),
    })
}
pub(crate) fn edits(
    module: &Module,
    request: &crate::Request<'_>,
    path: &Path,
) -> Result<Vec<TextEdit>, String> {
    json_items(module, request, path, Hook::Format, |item| {
        serde_json::from_value(item).map_err(|e| e.to_string())
    })
}

/// Feature modules own the invocation kind: a control a module proposed that
/// runs its reducer when executed. The reducer answers with the concrete
/// action the invocation stands for, which its own owner then prepares.
pub(crate) struct Invocations;
impl ActionProvider for Invocations {
    fn kinds(&self) -> &'static [CommandId] {
        &[CommandId::Invoke]
    }
    fn prepare(
        &self,
        request: &crate::Request<'_>,
        action: &Action,
        capabilities: Capabilities,
    ) -> Result<Prepared, String> {
        let Action::Invoke { event, .. } = action else {
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
/// The module an invocation still reaches, with the document it acts on.
fn invocation<'a>(
    request: &crate::Request<'a>,
    action: &Action,
) -> Result<(PathBuf, &'a Module), String> {
    let Action::Invoke {
        document,
        expected,
        module,
        revision,
        ..
    } = action
    else {
        return Err("Expected a module invocation".into());
    };
    let (path, doc) = commands::document(request, document)?;
    if doc.text != *expected {
        return Err(SOURCE_CHANGED.into());
    }
    let module = request
        .workspace()
        .modules()
        .get(module)
        .ok_or("Module is no longer available")?;
    if module.revision() != *revision {
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
    let context =
        input(module, request, &mut request.engine(), path)?.with_capabilities(capabilities);
    let result = module.call(
        Hook::Reduce,
        vec![context.to_value(), from_json(event)],
        request.now(),
    )?;
    let action: Action = serde_json::from_value(json(&result)?).map_err(|e| e.to_string())?;
    if matches!(action, Action::Invoke { .. }) {
        return Err("A reducer must return a concrete action".into());
    }
    Ok(action)
}
