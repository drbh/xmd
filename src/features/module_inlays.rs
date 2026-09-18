//! Modules consume the same semantic records as queries and emit validated data.
use crate::{
    catalog::{self, QueryContext},
    commands::{Action, Capabilities},
    engine::{Engine, Value},
    inlays::{InlayContext, InlayFeature, InlaySink},
    modules::{Hook, Module, ModuleKind, from_json, json, record},
};
use lsp_types::{Command, Position, Range, TextEdit};
use std::path::Path;

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
pub(crate) fn input(
    module: &Module,
    engine: &mut Engine<'_>,
    path: &Path,
) -> Result<Value, String> {
    let doc = &engine.workspace.documents[path];
    let Value::Record(mut document) = object([
        ("path", Value::Text(path.to_string_lossy().into())),
        ("uri", Value::Text(crate::paths::file_url(path)?.into())),
        ("text", Value::Text(doc.text.clone())),
        (
            "lines",
            Value::List(doc.text.lines().map(|s| Value::Text(s.into())).collect()),
        ),
    ]) else {
        unreachable!()
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
            document.insert("definitions".into(), definitions);
        }
        document.insert(collection.as_str().into(), values);
    }
    Ok(object([
        ("today", Value::Date(engine.today)),
        ("document", Value::Record(document)),
        (
            "module",
            object([
                ("id", Value::Text(module.id.clone())),
                ("revision", Value::Text(module.revision())),
            ]),
        ),
    ]))
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
            let mut input = input(self, context.engine, context.path)?;
            if let Value::Record(fields) = &mut input {
                fields.insert("range".into(), from_json(&serde_json::json!(context.range)));
            }
            if self.has(Hook::TimeDependent) {
                if self.call(Hook::TimeDependent, vec![input.clone()], context.engine.now)?
                    == Value::Bool(true)
                {
                    context.mark_time_dependent();
                }
            } else if self.live {
                context.mark_time_dependent();
            }
            let Value::List(hints) = self.call(Hook::Collect, vec![input], context.engine.now)?
            else {
                return Err("collect must return a list".into());
            };
            let mut validated = Vec::new();
            for hint in hints {
                let Value::Record(fields) = hint else {
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
                validate_position(&document.text, position)?;
                let Some(Value::Text(label)) = fields.get("label") else {
                    return Err("Inlay label must be text".into());
                };
                let tooltip = match fields.get("tooltip") {
                    None => String::new(),
                    Some(Value::Text(s)) => s.clone(),
                    _ => return Err("Inlay tooltip must be text".into()),
                };
                validated.push((position, label.clone(), tooltip));
            }
            Ok::<_, String>(validated)
        })();
        match result {
            Ok(hints) => {
                for (position, label, tooltip) in hints {
                    output.push(position, label, tooltip);
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
pub fn commands(
    request: &crate::RequestContext<'_>,
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
            let Value::Record(mut ctx) = input(module, &mut engine, path)? else {
                unreachable!()
            };
            ctx.insert("row".into(), Value::Count(row));
            ctx.insert(
                "capabilities".into(),
                object([
                    ("refresh", Value::Bool(capabilities.refresh)),
                    ("views", Value::Bool(capabilities.views)),
                ]),
            );
            let Value::List(proposals) =
                module.call(Hook::Actions, vec![Value::Record(ctx)], request.now())?
            else {
                return Err("actions must return a list".into());
            };
            let mut validated = vec![];
            for proposal in proposals {
                let Value::Record(fields) = proposal else {
                    return Err("Each action must be a record".into());
                };
                let Some(Value::Text(title)) = fields.get("title") else {
                    return Err("Action title must be text".into());
                };
                let action: Action =
                    serde_json::from_value(json(fields.get("action").ok_or("Missing action")?)?)
                        .map_err(|e| e.to_string())?;
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
    request: &crate::RequestContext<'_>,
    path: &Path,
    module: &Module,
    event: &serde_json::Value,
    capabilities: Capabilities,
) -> Result<Action, String> {
    let Value::Record(mut context) = input(module, &mut request.engine(), path)? else {
        unreachable!()
    };
    context.insert(
        "capabilities".into(),
        object([
            ("refresh", Value::Bool(capabilities.refresh)),
            ("views", Value::Bool(capabilities.views)),
        ]),
    );
    let result = module.call(
        Hook::Reduce,
        vec![Value::Record(context), from_json(event)],
        request.now(),
    )?;
    let action: Action = serde_json::from_value(json(&result)?).map_err(|e| e.to_string())?;
    if matches!(action, Action::Invoke { .. }) {
        return Err("A reducer must return a concrete action".into());
    }
    Ok(action)
}
