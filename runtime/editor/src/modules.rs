//! A .xmd feature module as a provider: it consumes the same semantic records
//! as queries and returns data only, and this adapter validates every
//! position and edit it proposes.
use crate::{
    commands::{Action, Capabilities},
    inlays::{InlayContext, InlaySink},
};
use chrono::{DateTime, FixedOffset, NaiveDate};
use lang::eval::RequestContext;
use lang::eval::engine::{Engine, Value};
use lang::eval::modules::{Collection, Hook, Module, ModuleKind, from_json, json};
use lang::eval::{ToValue, record};
use lang::model::Document;
use lsp_types::{
    Command, Diagnostic, DiagnosticSeverity, Hover, HoverContents, MarkupContent, MarkupKind,
    NumberOrString, Position, Range, TextEdit,
};
use std::{collections::BTreeMap, path::Path};

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
    row: Option<usize>,
    capabilities: Option<Capabilities>,
}
impl HookInput {
    const TODAY: &'static str = "today";
    const DOCUMENT: &'static str = "document";
    const MODULE: &'static str = "module";
    const RANGE: &'static str = "range";
    const ROW: &'static str = "row";
    const CAPABILITIES: &'static str = "capabilities";
    fn with_range(mut self, range: Range) -> Self {
        self.range = Some(range);
        self
    }
    fn with_row(mut self, row: usize) -> Self {
        self.row = Some(row);
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
        // The optional three are absent, not null, when the call has no use
        // for them: a hook tells them apart with `has`.
        if let Some(range) = self.range {
            fields.insert(Self::RANGE.into(), from_json(&serde_json::json!(range)));
        }
        if let Some(row) = self.row {
            fields.insert(Self::ROW.into(), row.to_value());
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
        Value::Record(fields)
    }
}

/// One inlay a module proposed, with its position already resolved against the
/// document and validated.
struct InlayHint {
    position: Position,
    label: String,
    tooltip: String,
}
impl TryFrom<(&Document, Value)> for InlayHint {
    type Error = String;
    fn try_from((document, value): (&Document, Value)) -> Result<Self, String> {
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
            if line >= document.text.lines().count() {
                return Err("Inlay line is outside the document".into());
            }
            document.line_end(line)
        };
        validate(&document.text, Range::new(position, position))?;
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

/// One action a module proposed, before its capabilities and source are checked.
struct ActionProposal {
    title: String,
    action: Action,
}
impl TryFrom<Value> for ActionProposal {
    type Error = String;
    fn try_from(value: Value) -> Result<Self, String> {
        let Value::Record(fields) = value else {
            return Err("Each action must be a record".into());
        };
        let Some(Value::Text(title)) = fields.get("title") else {
            return Err("Action title must be text".into());
        };
        let action: Action =
            serde_json::from_value(json(fields.get("action").ok_or("Missing action")?)?)
                .map_err(|e| e.to_string())?;
        Ok(Self {
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

pub(crate) fn input(
    module: &Module,
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
        let records = catalog::collect(
            engine.workspace(),
            *collection,
            engine,
            Some(path),
            |request, path| analysis::collect_native(request, path, false),
        )?;
        let values = Value::List(
            records
                .into_iter()
                .map(|mut r| {
                    if let Some(fields) = module.fields.get(collection) {
                        fields
                            .iter()
                            .map(|key| Ok((key.clone(), r.field(key, engine)?)))
                            .collect::<Result<_, String>>()
                            .map(Value::Record)
                    } else {
                        Ok(r.materialize(engine))
                    }
                })
                .collect::<Result<_, String>>()?,
        );
        if *collection == Collection::Values {
            // Preserve the original API's alias; all new fields come from the catalog.
            let mut definitions = values.clone();
            if let Value::List(items) = &mut definitions {
                for item in items {
                    if let Value::Record(fields) = item {
                        let error = match fields.get("errors") {
                            Some(Value::List(errors)) => {
                                errors.first().cloned().unwrap_or(Value::Null)
                            }
                            _ => Value::Null,
                        };
                        fields.insert("error".into(), error);
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
        row: None,
        capabilities: None,
    })
}

/// Whether `range` lies within `text` on UTF-16 boundaries.
fn validate(text: &str, range: Range) -> Result<(), String> {
    lang::model::apply_edits(text, &[TextEdit::new(range, String::new())]).map(|_| ())
}

fn document_text<'a>(request: &RequestContext<'a>, path: &Path) -> &'a str {
    &request.workspace().documents()[path].text
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
    items.into_iter().map(decode).collect()
}
/// Call one data-only hook, when the module defines it, and decode its items
/// once every one of them has converted to JSON.
fn json_items<T>(
    module: &Module,
    request: &RequestContext<'_>,
    path: &Path,
    hook: Hook,
    decode: impl FnMut(serde_json::Value) -> Result<T, String>,
) -> Result<Vec<T>, String> {
    if !module.has(hook) {
        return Ok(vec![]);
    }
    let input = input(module, &mut request.engine(), path)?;
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
    let result = input(module, context.engine, context.path).and_then(|input| {
        let input = input.with_range(context.range);
        let now = context.engine.now();
        if module.has(Hook::TimeDependent) {
            if module.call(Hook::TimeDependent, vec![input.to_value()], now)? == Value::Bool(true) {
                context.mark_time_dependent();
            }
        } else if module.live {
            context.mark_time_dependent();
        }
        hook_items(module, Hook::Collect, &input, now, |hint| {
            InlayHint::try_from((document, hint))
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
/// What a module's `actions` hook is handed for every row but the row itself,
/// or nothing when it has no such hook.
pub(crate) fn actions_input(
    module: &Module,
    request: &RequestContext<'_>,
    path: &Path,
    capabilities: Capabilities,
) -> Option<Result<HookInput, String>> {
    module
        .has(Hook::Actions)
        .then(|| Ok(input(module, &mut request.engine(), path)?.with_capabilities(capabilities)))
}
/// Actions are proposals. Preparation validates capabilities and source
/// before exposing controls; the host repeats validation against the
/// execution snapshot.
pub(crate) fn controls(
    module: &Module,
    input: &Result<HookInput, String>,
    request: &RequestContext<'_>,
    row: usize,
    capabilities: Capabilities,
) -> Vec<Command> {
    let Ok(input) = input else {
        return vec![];
    };
    let input = input.clone().with_row(row);
    hook_items(module, Hook::Actions, &input, request.now(), |proposal| {
        let ActionProposal { title, action } = ActionProposal::try_from(proposal)?;
        if !capabilities.supports(&action) {
            return Ok(None);
        }
        if matches!(action, Action::Invoke { .. }) {
            action.validate_invocation(request)?;
        } else {
            action.prepare(request, capabilities)?;
        }
        Ok(Some(action.command(title)))
    })
    .map(|commands| commands.into_iter().flatten().collect())
    .unwrap_or_default()
}
/// A hook that fails is itself a diagnostic, naming the module.
pub(crate) fn diagnostics(
    module: &Module,
    request: &RequestContext<'_>,
    path: &Path,
) -> Vec<Diagnostic> {
    json_items(module, request, path, Hook::Diagnostics, |item| {
        let mut diagnostic: Diagnostic = serde_json::from_value(item).map_err(|e| e.to_string())?;
        validate(document_text(request, path), diagnostic.range)?;
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
    request: &RequestContext<'_>,
    path: &Path,
    at: Position,
) -> Option<Hover> {
    json_items(module, request, path, Hook::Hovers, |item| {
        let hover = HoverItem::try_from(item)?;
        validate(document_text(request, path), hover.range)?;
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
    request: &RequestContext<'_>,
    path: &Path,
) -> Result<Vec<TextEdit>, String> {
    json_items(module, request, path, Hook::Format, |item| {
        serde_json::from_value(item).map_err(|e| e.to_string())
    })
}

pub(crate) fn reduce(
    request: &RequestContext<'_>,
    path: &Path,
    module: &Module,
    event: &serde_json::Value,
    capabilities: Capabilities,
) -> Result<Action, String> {
    let context = input(module, &mut request.engine(), path)?.with_capabilities(capabilities);
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
