use crate::controls::code_actions;
use crate::controls::commands::{Action, Capabilities, RowTarget};
use eval::engine::{Engine, Value};
use eval::modules::{Hook, ModuleKind};
use eval::resources::{Resource, ResourcePresenting};
use lsp_types::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(crate) fn resources_at(
    request: &eval::RequestContext<'_>,
    path: &Path,
    row: usize,
) -> Vec<Resource> {
    let ws = request.workspace();

    let Some(doc) = ws.documents.get(path) else {
        return vec![];
    };
    let mut engine = request.engine();
    let mut found = BTreeMap::new();
    let mut add = |resource: Resource| {
        if let Ok(url) = resource.url(path) {
            found.insert(url.to_string(), resource);
        }
    };
    for name in doc
        .definitions
        .iter()
        .filter(|d| d.named.span.line == row)
        .map(|d| d.named.name.as_str())
        .chain(
            doc.references
                .iter()
                .filter(|r| r.span.line == row)
                .map(|r| r.name.as_str()),
        )
    {
        if let Ok(Value::Resource(r)) = engine.named(path, name) {
            add(r);
        }
    }
    for link in doc.links.iter().filter(|l| l.span.line == row) {
        if let Some(mut r) = Resource::parse(&link.target) {
            r.origin = Some(path.into());
            add(r);
        }
    }
    found.into_values().collect()
}
/// The shared title for the task toggle lens and code action.
pub(crate) fn task_toggle_title(engine: &mut Engine<'_>, path: &Path, index: usize) -> String {
    let recurring = engine.workspace().documents[path].tasks[index]
        .attributes
        .contains_key("every");
    let done = engine.task_done(path, index);
    engine.present(
        "task",
        "toggle",
        vec![Value::Bool(recurring), Value::Bool(done)],
    )
}
/// A control title: a `format.glyph` and the one word that disambiguates it.
pub(crate) fn titled(engine: &mut Engine<'_>, glyph: &str, word: &str) -> String {
    let glyph = engine.present("format", "glyph", vec![Value::Text(glyph.into())]);
    format!("{glyph} {word}")
}
pub(crate) fn builtin_controls(
    request: &eval::RequestContext<'_>,
    path: &Path,
    row: usize,
    include_task: bool,
    capabilities: Capabilities,
) -> Vec<Command> {
    let ws = request.workspace();
    let Some(doc) = ws.documents.get(path) else {
        return vec![];
    };
    let uri = common::file_url(path).unwrap();
    let target = RowTarget {
        document: uri.clone(),
        row,
        expected: doc.line(row).into(),
    };
    let mut result = vec![];
    let mut push = |action: Action, title: String| {
        if capabilities.supports(&action) {
            result.push(action.command(title));
        }
    };
    let mut engine = request.engine();
    if include_task
        && let Some(index) = doc.tasks.iter().position(|t| t.line == row)
        && code_actions::toggle_task(request, path, index).is_ok()
    {
        let title = task_toggle_title(&mut engine, path, index);
        push(Action::ToggleTask(target.clone()), title);
    }
    for resource in resources_at(request, path, row) {
        let url = resource.url(path).unwrap();
        let title = engine.present("resource", "control", vec![resource.record(path)]);
        push(
            Action::OpenResource {
                target: target.clone(),
                url: url.clone(),
            },
            title,
        );
        if let Some(refresh) = request.link_features().refresh_request(&resource.target) {
            push(
                Action::RefreshResource {
                    target: target.clone(),
                    url,
                },
                refresh.title,
            );
        }
    }
    let line = doc.line(row);
    let wants_lookup = ["rate(", "to(", "forecast(", "forecast_range(", "quote("]
        .iter()
        .any(|call| line.contains(call))
        || doc.days.iter().any(|d| d.line == row && d.places.is_some());
    if wants_lookup {
        push(
            Action::Refresh {
                document: Some(uri),
            },
            titled(&mut engine, "refresh", "lookups"),
        );
    }
    result
}
pub(crate) fn lenses(
    request: &eval::RequestContext<'_>,
    path: &Path,
    capabilities: Capabilities,
) -> Vec<CodeLens> {
    let ws = request.workspace();

    let Some(doc) = ws.documents.get(path) else {
        return vec![];
    };
    let rows: BTreeSet<_> = doc
        .definitions
        .iter()
        .map(|d| d.named.span.line)
        .chain(doc.tasks.iter().map(|t| t.line))
        .chain(doc.references.iter().map(|r| r.span.line))
        .chain(doc.links.iter().map(|l| l.span.line))
        .chain(
            if ws
                .modules
                .active()
                .any(|m| m.kind == ModuleKind::Feature && m.has(Hook::Actions))
            {
                0..doc.text.lines().count()
            } else {
                0..0
            },
        )
        .collect();
    rows.into_iter()
        .flat_map(|row| {
            crate::providers::controls(request, path, row, true, capabilities)
                .into_iter()
                .map(move |command| CodeLens {
                    range: Range::new(Position::new(row as u32, 0), Position::new(row as u32, 0)),
                    command: Some(command),
                    data: None,
                })
        })
        .collect()
}
