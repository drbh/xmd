//! Built-in inlay producers. Each implementation shares the same request context and sink.
use crate::{
    document::Span,
    engine::Value,
    inlays::{InlayContext, InlayFeature, InlaySink},
    workspace::{Symbol, SymbolKind},
};

/// Stable registration order breaks ties between hints at the same position.
pub const BUILTINS: &[&dyn InlayFeature] = &[
    &DefinitionInlays,
    &TaskInlays,
    &ReferenceInlays,
    &LinkInlays,
    &super::plugin_inlays::PluginInlays,
];

pub struct DefinitionInlays;
impl InlayFeature for DefinitionInlays {
    fn id(&self) -> &str {
        "definitions"
    }
    fn collect(&self, context: &mut InlayContext<'_, '_>, output: &mut InlaySink) {
        let engine = &mut *context.engine;
        let path = context.path;
        let doc = context.document;
        let mut push = |position, label, tooltip| output.push(position, label, tooltip);
        for (i, def) in doc.definitions.iter().enumerate() {
            match engine.symbol(&Symbol {
                path: path.to_path_buf(),
                kind: SymbolKind::Definition(i),
            }) {
                // Resource values are rendered by the LinkInlays adapter.
                Ok(Value::Resource(_) | Value::Plan(_) | Value::Timer(_)) => {}
                Ok(value) if def.expression => push(
                    def.end.range(&doc.text).start,
                    format!("= {}", value.display()),
                    crate::intelligence::hover_in(
                        &engine.request(),
                        &Symbol {
                            path: path.into(),
                            kind: SymbolKind::Definition(i),
                        },
                    ),
                ),
                _ => {}
            }
        }
    }
}

pub struct TaskInlays;
impl InlayFeature for TaskInlays {
    fn id(&self) -> &str {
        "tasks"
    }
    fn collect(&self, context: &mut InlayContext<'_, '_>, output: &mut InlaySink) {
        let engine = &mut *context.engine;
        let path = context.path;
        let doc = context.document;
        let today = engine.today;
        let mut push = |position, label, tooltip| output.push(position, label, tooltip);
        for (i, task) in doc.tasks.iter().enumerate() {
            let mut labels = Vec::new();
            if let Some(attr) = task.attributes.get("timer")
                && let Ok(Value::Timer(timer)) = engine.eval(path, &attr.value)
            {
                labels.push(timer.inlay());
            }
            if !engine.task_done(path, i) {
                match engine.blocked(path, i) {
                    Ok(blocked) if !blocked.is_empty() => labels.push(format!(
                        "{} blocked by {}",
                        crate::glyphs::BLOCKED,
                        blocked.join(", ")
                    )),
                    Err(e) => labels.push(e),
                    _ => {}
                }
                for key in ["due", "scheduled", "at"] {
                    if let Some(attr) = task.attributes.get(key) {
                        match engine.when(path, &attr.value) {
                            Ok(value) => {
                                let date = engine.date(&value).unwrap();
                                let delta = (date - today).num_days();
                                let relative = if delta < 0 && key == "due" {
                                    format!("{} {}d overdue", crate::glyphs::ALERT, -delta)
                                } else if delta == 0 {
                                    "today".into()
                                } else if delta == 1 {
                                    "tomorrow".into()
                                } else {
                                    date.to_string()
                                };
                                labels.push(format!("{} {key} {relative}", crate::glyphs::ARROW));
                            }
                            Err(e) => labels.push(e),
                        }
                    }
                }
                if let Some(attr) = task.attributes.get("every") {
                    labels.push(format!("{} every {}", crate::glyphs::REPEAT, attr.value));
                }
            }
            let children: Vec<_> = doc
                .tasks
                .iter()
                .enumerate()
                .filter(|(_, t)| t.parent == Some(i))
                .collect();
            let mut tooltip = String::new();
            if !children.is_empty() {
                let done = children
                    .iter()
                    .filter(|(j, _)| engine.task_done(path, *j))
                    .count();
                labels.push(format!(
                    "{} {done}/{} subtasks",
                    crate::charts::gauge(done, children.len()),
                    children.len()
                ));
                tooltip = format!(
                    "`{}` {done}/{} subtasks\n\n",
                    crate::charts::bar(done, children.len()),
                    children.len()
                );
            }
            if !labels.is_empty() {
                tooltip.push_str("Use code actions to complete/reopen tasks or start/pause/reset their timer. Completing a task does not stop its timer.");
                push(doc.line_end(task.line), labels.join(" · "), tooltip);
            }
        }
    }
}

pub struct ReferenceInlays;
impl InlayFeature for ReferenceInlays {
    fn id(&self) -> &str {
        "references"
    }
    fn collect(&self, context: &mut InlayContext<'_, '_>, output: &mut InlaySink) {
        let engine = &mut *context.engine;
        let workspace = engine.workspace;
        let path = context.path;
        let doc = context.document;
        let mut push = |position, label, tooltip| output.push(position, label, tooltip);
        for reference in doc.references.iter().filter(|r| r.bracket) {
            if let Ok(value) = engine.eval(path, &reference.expression()) {
                let end = reference.end()
                    + doc.line(reference.span.line)[reference.end()..]
                        .find(']')
                        .unwrap_or(0)
                    + 1;
                let after = Span::new(reference.span.line, end, end)
                    .range(&doc.text)
                    .start;
                match value {
                    Value::Resource(_) => {}
                    Value::Timer(_) => {}
                    value if reference.property.is_some() => {
                        push(after, value.display(), reference.expression());
                    }
                    value => {
                        let tooltip = workspace
                            .resolve(path, &reference.name)
                            .map(|symbol| crate::intelligence::hover_in(&engine.request(), &symbol))
                            .unwrap_or_else(|_| reference.expression());
                        push(after, value.display(), tooltip);
                    }
                }
            }
        }
    }
}

/// Adapts URL semantics to prose links, definitions and references in the shared pipeline.
/// Providers need no knowledge of definitions, references, UTF-16 or editor hosts.
pub struct LinkInlays;
impl InlayFeature for LinkInlays {
    fn id(&self) -> &str {
        "links"
    }
    fn collect(&self, context: &mut InlayContext<'_, '_>, output: &mut InlaySink) {
        let engine = &mut *context.engine;
        let workspace = engine.workspace;
        let path = context.path;
        let doc = context.document;
        let now = engine.now.to_utc();
        let features = engine.link_features();
        let mut time_dependent = false;
        let mut push = |resource: &crate::resources::Resource, position, only_known| {
            let view = resource.presentation(path, &workspace.cache, now, features);
            if only_known && !view.known_link {
                return;
            }
            time_dependent |= view.time_dependent;
            output.push(position, view.label, view.hover);
        };
        for (i, def) in doc.definitions.iter().enumerate() {
            if let Ok(Value::Resource(resource)) = engine.symbol(&Symbol {
                path: path.into(),
                kind: SymbolKind::Definition(i),
            }) {
                push(&resource, def.end.range(&doc.text).start, false);
            }
        }
        for reference in doc.references.iter().filter(|r| r.bracket) {
            if let Ok(Value::Resource(resource)) = engine.eval(path, &reference.expression()) {
                let end = reference.end()
                    + doc.line(reference.span.line)[reference.end()..]
                        .find(']')
                        .unwrap_or(0)
                    + 1;
                let position = Span::new(reference.span.line, end, end)
                    .range(&doc.text)
                    .start;
                push(&resource, position, false);
            }
        }
        // Prose links get badges only when a provider recognizes them.
        for link in &doc.links {
            let resource = crate::resources::Resource {
                target: link.target.clone(),
                origin: Some(path.into()),
            };
            push(&resource, link.span.range(&doc.text).end, true);
        }
        engine.time_dependent |= time_dependent;
    }
}
