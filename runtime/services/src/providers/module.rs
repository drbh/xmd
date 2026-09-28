//! A .x.md feature module as a provider: it consumes the same semantic records
//! as queries and returns data only, and this adapter validates every
//! position and edit it proposes.
use super::Provider;
use crate::{
    controls::commands::{Action, Capabilities},
    data::catalog::{self, QueryContext},
    view::inlays::{InlayContext, InlaySink},
};
use chrono::NaiveDate;
use lang::eval::RequestContext;
use lang::eval::engine::{Engine, Value};
use lang::eval::modules::{Hook, Module, ModuleKind, from_json, json};
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
    let doc = &engine.workspace().documents[path];
    let mut document = DocumentInput {
        path: path.to_string_lossy().into(),
        uri: lang::common::file_url(path)?.into(),
        text: doc.text.clone(),
        lines: doc.text.lines().map(str::to_owned).collect(),
        collections: BTreeMap::new(),
    };
    for collection in &module.inputs {
        let records = catalog::collect_document(
            engine.workspace(),
            *collection,
            QueryContext::new(engine.now()),
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
                            .map(|key| Ok((key.clone(), r.field(key, engine)?)))
                            .collect::<Result<_, String>>()
                            .map(Value::Record)
                    } else {
                        Ok(r.materialize(engine))
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
                .insert("definitions".into(), definitions);
        }
        document
            .collections
            .insert(collection.as_str().into(), values);
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

fn validate_position(text: &str, position: Position) -> Result<(), String> {
    crate::controls::code_actions::apply_edits(
        text,
        &[TextEdit {
            range: Range::new(position, position),
            new_text: String::new(),
        }],
    )
    .map(|_| ())
}
/// A feature module is a provider: each hook it defines answers for one kind.
impl Provider for Module {
    fn inlays(&self, context: &mut InlayContext<'_, '_>, output: &mut InlaySink) {
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
                        context.engine.now(),
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
                .call(Hook::Collect, vec![input.to_value()], context.engine.now())
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
    /// Actions are proposals. Preparation validates capabilities and source
    /// before exposing controls; the host repeats validation against the
    /// execution snapshot.
    fn controls(
        &self,
        request: &RequestContext<'_>,
        path: &Path,
        row: usize,
        _include_task: bool,
        capabilities: Capabilities,
    ) -> Vec<Command> {
        if !self.has(Hook::Actions) {
            return vec![];
        }
        let result = (|| {
            let input = input(self, &mut request.engine(), path)?
                .with_row(row)
                .with_capabilities(capabilities);
            let Value::List(proposals) = self
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
        result.unwrap_or_default()
    }
    /// A hook that fails is itself a diagnostic, naming the module.
    fn diagnostics(
        &self,
        request: &RequestContext<'_>,
        path: &Path,
        _editing: bool,
    ) -> Vec<Diagnostic> {
        if !self.has(Hook::Diagnostics) {
            return vec![];
        }
        let batch = (|| {
            let mut batch = vec![];
            for item in call(request, path, self, Hook::Diagnostics)? {
                let mut diagnostic: Diagnostic =
                    serde_json::from_value(item).map_err(|e| e.to_string())?;
                validate(request, path, diagnostic.range)?;
                diagnostic.source.get_or_insert("xmd".into());
                batch.push(diagnostic);
            }
            Ok::<_, String>(batch)
        })();
        batch.unwrap_or_else(|error| {
            vec![Diagnostic {
                range: Range::default(),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("xmd".into()),
                code: Some(NumberOrString::String("module".into())),
                message: format!("{}: {error}", self.id),
                ..Default::default()
            }]
        })
    }
    fn hover(&self, request: &RequestContext<'_>, path: &Path, at: Position) -> Option<Hover> {
        if !self.has(Hook::Hovers) {
            return None;
        }
        let batch = (|| {
            let mut batch = vec![];
            for item in call(request, path, self, Hook::Hovers)? {
                let hover = HoverItem::try_from(item)?;
                validate(request, path, hover.range)?;
                batch.push(hover);
            }
            Ok::<_, String>(batch)
        })();
        batch
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
    fn edits(&self, request: &RequestContext<'_>, path: &Path) -> Result<Vec<TextEdit>, String> {
        if !self.has(Hook::Format) {
            return Ok(vec![]);
        }
        call(request, path, self, Hook::Format)?
            .into_iter()
            .map(|value| serde_json::from_value(value).map_err(|e| e.to_string()))
            .collect()
    }
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
    crate::controls::code_actions::apply_edits(
        &request.workspace().documents[path].text,
        &[TextEdit::new(range, String::new())],
    )
    .map(|_| ())
}
