//! The generic inlay adapter for functional modules.
use crate::{
    engine::Value,
    inlays::{InlayContext, InlayFeature, InlaySink},
    plugins::{Module, record},
    workspace::{Symbol, SymbolKind},
};

pub struct PluginInlays;
impl InlayFeature for PluginInlays {
    fn collect(&self, context: &mut InlayContext<'_, '_>, output: &mut InlaySink) {
        let plugins = context.engine.workspace.plugins.clone();
        for module in plugins.modules.iter().filter(|m| m.kind == "inlay") {
            module.collect(context, output);
        }
    }
}
fn object<const N: usize>(fields: [(&str, Value); N]) -> Value {
    record(fields.into_iter().map(|(k, v)| (k.into(), v)))
}
impl InlayFeature for Module {
    fn collect(&self, context: &mut InlayContext<'_, '_>, output: &mut InlaySink) {
        if self.kind != "inlay" {
            return;
        }
        let document = context.document;
        let sections = document
            .sections
            .iter()
            .map(|s| {
                object([
                    ("line", Value::Count(s.line)),
                    ("title", Value::Text(s.title.clone())),
                ])
            })
            .collect();
        let tasks = document
            .tasks
            .iter()
            .map(|t| {
                object([
                    ("line", Value::Count(t.line)),
                    ("title", Value::Text(t.title.clone())),
                    ("checked", Value::Bool(t.checked)),
                ])
            })
            .collect();
        let links = document
            .links
            .iter()
            .map(|l| {
                object([
                    ("line", Value::Count(l.span.line)),
                    ("url", Value::Text(l.target.clone())),
                ])
            })
            .collect();
        let definitions = document
            .definitions
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let evaluated = context.engine.symbol(&Symbol {
                    path: context.path.into(),
                    kind: SymbolKind::Definition(i),
                });
                let (value, error) = match evaluated {
                    Ok(v) => (v, Value::Null),
                    Err(e) => (Value::Null, Value::Text(e)),
                };
                object([
                    ("line", Value::Count(d.named.span.line)),
                    ("name", Value::Text(d.named.name.clone())),
                    ("value", value),
                    ("error", error),
                ])
            })
            .collect();
        let input = object([(
            "document",
            object([
                ("path", Value::Text(context.path.to_string_lossy().into())),
                (
                    "lines",
                    Value::List(
                        document
                            .text
                            .lines()
                            .map(|l| Value::Text(l.into()))
                            .collect(),
                    ),
                ),
                ("sections", Value::List(sections)),
                ("tasks", Value::List(tasks)),
                ("links", Value::List(links)),
                ("definitions", Value::List(definitions)),
            ]),
        )]);
        let result = (|| {
            let Value::List(hints) = self.call("collect", vec![input], context.engine.now)? else {
                return Err("collect must return a list".into());
            };
            let mut validated = Vec::new();
            for hint in hints {
                let Value::Record(fields) = hint else {
                    return Err("Each inlay must be a record".into());
                };
                let line = match fields.get("line") {
                    Some(Value::Count(n)) => *n,
                    Some(Value::Number(n)) if n.is_finite() && *n >= 0.0 && n.fract() == 0.0 => {
                        *n as usize
                    }
                    _ => return Err("Inlay line must be a nonnegative integer".into()),
                };
                if line >= document.text.lines().count() {
                    return Err("Inlay line is outside the document".into());
                }
                let Some(Value::Text(label)) = fields.get("label") else {
                    return Err("Inlay label must be text".into());
                };
                let tooltip = match fields.get("tooltip") {
                    None => String::new(),
                    Some(Value::Text(s)) => s.clone(),
                    _ => return Err("Inlay tooltip must be text".into()),
                };
                validated.push((document.line_end(line), label.clone(), tooltip));
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
                format!("plugin error · {}", self.id),
                error,
            ),
        }
        if self.live {
            context.mark_time_dependent();
        }
    }
}
