//! Feature modules: how .wtf feature modules drive inlays, hovers, diagnostics,
//! formatting and actions. They consume the same semantic records as queries and
//! return data only; this adapter validates every position and edit they propose.
use crate::{
    RequestContext,
    catalog::{self, QueryContext},
    commands::{Action, Capabilities},
    document::Document,
    engine::{Engine, Value},
    inlays::{InlayContext, InlayFeature, InlaySink},
    modules::{Hook, Module, ModuleKind, from_json, json, record},
};
use chrono::NaiveDate;
use lsp_types::{
    Command, Diagnostic, DiagnosticSeverity, Hover, HoverContents, MarkupContent, MarkupKind,
    NumberOrString, Position, Range, TextEdit,
};
use std::path::Path;

/// The default inlay pipeline: every enabled feature module, in order.
pub const BUILTINS: &[&dyn InlayFeature] = &[&ModuleInlays];

/// Runs each feature module's `collect` hook as one inlay producer.
pub struct ModuleInlays;
impl InlayFeature for ModuleInlays {
    fn collect(&self, context: &mut InlayContext<'_, '_>, output: &mut InlaySink) {
        let modules = context.engine.workspace.modules.clone();
        for module in modules.active().filter(|m| m.kind == ModuleKind::Feature) {
            module.collect(context, output);
        }
    }
}
fn object<const N: usize>(fields: [(&str, Value); N]) -> Value {
    record(fields.into_iter().map(|(k, v)| (k.into(), v)))
}

/// The note a hook is looking at, with the record collections it declared.
struct DocumentInput {
    path: String,
    uri: String,
    text: String,
    lines: Vec<String>,
    collections: Vec<(String, Value)>,
}
impl DocumentInput {
    fn to_value(&self) -> Value {
        let Value::Record(mut fields) = object([
            ("path", Value::Text(self.path.clone())),
            ("uri", Value::Text(self.uri.clone())),
            ("text", Value::Text(self.text.clone())),
            (
                "lines",
                Value::List(self.lines.iter().cloned().map(Value::Text).collect()),
            ),
        ]) else {
            unreachable!()
        };
        fields.extend(self.collections.iter().cloned());
        Value::Record(fields)
    }
}

/// Everything a feature module's hook is handed. Built once per call, so the
/// context a module sees is one typed value rather than a record assembled
/// field by field at each call site.
pub(crate) struct HookInput {
    today: NaiveDate,
    document: DocumentInput,
    module_id: String,
    module_revision: String,
    range: Option<Range>,
    row: Option<usize>,
    capabilities: Option<Capabilities>,
}
impl HookInput {
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
    fn to_value(&self) -> Value {
        let Value::Record(mut fields) = object([
            ("today", Value::Date(self.today)),
            ("document", self.document.to_value()),
            (
                "module",
                object([
                    ("id", Value::Text(self.module_id.clone())),
                    ("revision", Value::Text(self.module_revision.clone())),
                ]),
            ),
        ]) else {
            unreachable!()
        };
        if let Some(range) = self.range {
            fields.insert("range".into(), from_json(&serde_json::json!(range)));
        }
        if let Some(row) = self.row {
            fields.insert("row".into(), Value::Count(row));
        }
        if let Some(capabilities) = self.capabilities {
            fields.insert(
                "capabilities".into(),
                object([
                    ("refresh", Value::Bool(capabilities.refresh)),
                    ("views", Value::Bool(capabilities.views)),
                ]),
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
            serde_json::from_value::<Position>(json(at).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?
        } else {
            let line = fields.get("line").ok_or("Inlay needs at or line")?;
            let line = json(line)
                .map_err(|e| e.to_string())?
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .ok_or("Inlay line must be a nonnegative integer")?;
            if line >= document.text.lines().count() {
                return Err("Inlay line is outside the document".into());
            }
            document.line_end(line)
        };
        validate_position(&document.text, position)?;
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
        let action: Action = serde_json::from_value(
            json(fields.get("action").ok_or("Missing action")?).map_err(|e| e.to_string())?,
        )
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
    let doc = &engine.workspace.documents[path];
    let mut document = DocumentInput {
        path: path.to_string_lossy().into(),
        uri: crate::paths::file_url(path)?.into(),
        text: doc.text.clone(),
        lines: doc.text.lines().map(str::to_owned).collect(),
        collections: Vec::new(),
    };
    for collection in &module.inputs {
        let records = catalog::collect_document(
            engine.workspace,
            *collection,
            QueryContext::new(engine.now),
            engine,
            Some(path),
            false,
        )?;
        let values = Value::List(
            records
                .into_iter()
                .map(|mut r| {
                    if let Some(fields) = module.fields.get(collection) {
                        fields
                            .iter()
                            .map(|key| Ok((key.clone(), r.field(key, engine)?.value())))
                            .collect::<Result<_, String>>()
                            .map(Value::Record)
                    } else {
                        Ok(r.materialize(engine).value())
                    }
                })
                .collect::<Result<_, String>>()?,
        );
        if *collection == catalog::Collection::Values {
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
                .push(("definitions".into(), definitions));
        }
        document
            .collections
            .push((collection.as_str().into(), values));
    }
    Ok(HookInput {
        today: engine.today,
        document,
        module_id: module.id.clone(),
        module_revision: module.revision(),
        range: None,
        row: None,
        capabilities: None,
    })
}

fn validate_position(text: &str, position: Position) -> Result<(), String> {
    crate::actions::apply_edits(
        text,
        &[TextEdit {
            range: Range::new(position, position),
            new_text: String::new(),
        }],
    )
    .map(|_| ())
}
impl InlayFeature for Module {
    fn id(&self) -> &str {
        &self.id
    }
    fn collect(&self, context: &mut InlayContext<'_, '_>, output: &mut InlaySink) {
        if !self.enabled || self.kind != ModuleKind::Feature || !self.has(Hook::Collect) {
            return;
        }
        let document = context.document;
        let result = (|| {
            let input = input(self, context.engine, context.path)?.with_range(context.range);
            if self.has(Hook::TimeDependent) {
                if self
                    .call(
                        Hook::TimeDependent,
                        vec![input.to_value()],
                        context.engine.now,
                    )
                    .map_err(|e| e.to_string())?
                    == Value::Bool(true)
                {
                    context.mark_time_dependent();
                }
            } else if self.live {
                context.mark_time_dependent();
            }
            let Value::List(hints) = self
                .call(Hook::Collect, vec![input.to_value()], context.engine.now)
                .map_err(|e| e.to_string())?
            else {
                return Err("collect must return a list".into());
            };
            hints
                .into_iter()
                .map(|hint| InlayHint::try_from((document, hint)))
                .collect::<Result<Vec<_>, String>>()
        })();
        match result {
            Ok(hints) => {
                for hint in hints {
                    output.push(hint.position, hint.label, hint.tooltip);
                }
            }
            Err(error) => output.push(
                document.line_end(0),
                format!("module error · {}", self.id),
                error,
            ),
        }
    }
}
/// Actions are proposals. Preparation validates capabilities and source before
/// exposing controls; the host repeats validation against the execution snapshot.
pub(crate) fn commands(
    request: &RequestContext<'_>,
    path: &Path,
    row: usize,
    capabilities: Capabilities,
) -> Vec<Command> {
    let mut engine = request.engine();
    let mut commands = vec![];
    for module in request
        .workspace()
        .modules
        .active()
        .filter(|m| m.kind == ModuleKind::Feature && m.has(Hook::Actions))
    {
        let result = (|| {
            let input = input(module, &mut engine, path)?
                .with_row(row)
                .with_capabilities(capabilities);
            let Value::List(proposals) = module
                .call(Hook::Actions, vec![input.to_value()], request.now())
                .map_err(|e| e.to_string())?
            else {
                return Err("actions must return a list".into());
            };
            let mut validated = vec![];
            for proposal in proposals {
                let ActionProposal { title, action } = ActionProposal::try_from(proposal)?;
                if !capabilities.supports(&action) {
                    continue;
                }
                if matches!(action, Action::Invoke { .. }) {
                    action.validate_invocation(request)?;
                } else {
                    action.prepare(request, capabilities)?;
                }
                validated.push(action.command(title));
            }
            Ok::<_, String>(validated)
        })();
        if let Ok(proposals) = result {
            commands.extend(proposals);
        }
    }
    commands
}

pub(crate) fn reduce(
    request: &RequestContext<'_>,
    path: &Path,
    module: &Module,
    event: &serde_json::Value,
    capabilities: Capabilities,
) -> Result<Action, String> {
    let context = input(module, &mut request.engine(), path)?.with_capabilities(capabilities);
    let result = module
        .call(
            Hook::Reduce,
            vec![context.to_value(), from_json(event)],
            request.now(),
        )
        .map_err(|e| e.to_string())?;
    let action: Action = serde_json::from_value(json(&result).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    if matches!(action, Action::Invoke { .. }) {
        return Err("A reducer must return a concrete action".into());
    }
    Ok(action)
}

/// Call one data-only hook and hand back its JSON items.
fn call(
    request: &RequestContext<'_>,
    path: &Path,
    module: &Module,
    hook: Hook,
) -> Result<Vec<serde_json::Value>, String> {
    let input = input(module, &mut request.engine(), path)?.to_value();
    let Value::List(items) = module
        .call(hook, vec![input], request.now())
        .map_err(|e| e.to_string())?
    else {
        return Err(format!("{hook} must return a list"));
    };
    items
        .iter()
        .map(|item| json(item).map_err(|e| e.to_string()))
        .collect()
}
fn validate(request: &RequestContext<'_>, path: &Path, range: Range) -> Result<(), String> {
    crate::actions::apply_edits(
        &request.workspace().documents[path].text,
        &[TextEdit::new(range, String::new())],
    )
    .map(|_| ())
}
pub(crate) fn diagnostics(request: &RequestContext<'_>, path: &Path) -> Vec<Diagnostic> {
    if !request.workspace().documents.contains_key(path) {
        return vec![];
    }
    let mut result = vec![];
    for module in request
        .workspace()
        .modules
        .active()
        .filter(|m| m.kind == ModuleKind::Feature && m.has(Hook::Diagnostics))
    {
        let batch = (|| {
            let mut batch = vec![];
            for item in call(request, path, module, Hook::Diagnostics)? {
                let mut diagnostic: Diagnostic =
                    serde_json::from_value(item).map_err(|e| e.to_string())?;
                validate(request, path, diagnostic.range)?;
                diagnostic.source.get_or_insert("wtf".into());
                batch.push(diagnostic);
            }
            Ok::<_, String>(batch)
        })();
        match batch {
            Ok(batch) => result.extend(batch),
            Err(error) => result.push(Diagnostic {
                range: Range::default(),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("wtf".into()),
                code: Some(NumberOrString::String("module".into())),
                message: format!("{}: {error}", module.id),
                ..Default::default()
            }),
        }
    }
    result
}
pub(crate) fn hover(
    request: &RequestContext<'_>,
    path: &Path,
    position: Position,
) -> Option<Hover> {
    request.workspace().documents.get(path)?;
    for module in request
        .workspace()
        .modules
        .active()
        .filter(|m| m.kind == ModuleKind::Feature && m.has(Hook::Hovers))
    {
        let batch = (|| {
            let mut batch = vec![];
            for item in call(request, path, module, Hook::Hovers)? {
                let hover = HoverItem::try_from(item)?;
                validate(request, path, hover.range)?;
                batch.push(hover);
            }
            Ok::<_, String>(batch)
        })();
        if let Ok(batch) = batch {
            for hover in batch {
                if position >= hover.range.start && position <= hover.range.end {
                    return Some(Hover {
                        range: Some(hover.range),
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: hover.contents,
                        }),
                    });
                }
            }
        }
    }
    None
}
pub(crate) fn formatting(
    request: &RequestContext<'_>,
    path: &Path,
) -> Result<Vec<TextEdit>, String> {
    let doc = request
        .workspace()
        .documents
        .get(path)
        .ok_or("Unknown document")?;
    let mut edits = crate::tables::formatting(doc);
    for module in request
        .workspace()
        .modules
        .active()
        .filter(|m| m.kind == ModuleKind::Feature && m.has(Hook::Format))
    {
        for value in call(request, path, module, Hook::Format)? {
            edits.push(serde_json::from_value(value).map_err(|e| e.to_string())?);
        }
    }
    crate::actions::apply_edits(&doc.text, &edits)?;
    edits.sort_by_key(|e| (e.range.start, e.range.end));
    Ok(edits)
}
