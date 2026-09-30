use crate::code_actions::{self, TaskToggle};
use crate::commands::{Action, Capabilities, RowTarget};
use crate::providers;
use lang::eval::engine::{Engine, Value};
use lang::eval::resources::{Resource, ResourcePresenting};
use lang::model::Document;
use lang::stdlib;
use lsp_types::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(crate) fn resources_at(
    request: &lang::eval::RequestContext<'_>,
    path: &Path,
    row: usize,
) -> Vec<Resource> {
    let ws = request.workspace();

    let Some(doc) = ws.documents().get(path) else {
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
    let recurring = engine.workspace().documents()[path].tasks[index]
        .attributes
        .contains_key(lang::syntax::AttributeKey::Every.as_str());
    let done = engine.task_done(path, index);
    stdlib::shown(stdlib::task::toggle(engine, recurring, done))
}
/// A control title: a `format.glyph` and the one word that disambiguates it.
pub(crate) fn titled(engine: &mut Engine<'_>, glyph: &str, word: &str) -> String {
    let glyph = stdlib::shown(stdlib::format::glyph(engine, glyph));
    format!("{glyph} {word}")
}
pub(crate) fn builtin_controls(
    request: &lang::eval::RequestContext<'_>,
    path: &Path,
    row: usize,
    toggle: TaskToggle,
    capabilities: Capabilities,
) -> Vec<Command> {
    let ws = request.workspace();
    let Some(doc) = ws.documents().get(path) else {
        return vec![];
    };
    let uri = lang::common::file_url(path).unwrap();
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
    if toggle == TaskToggle::Command
        && let Some(index) = doc.tasks.iter().position(|t| t.line == row)
        && code_actions::toggle_task(request, path, index).is_ok()
    {
        let title = task_toggle_title(&mut engine, path, index);
        push(Action::ToggleTask(target.clone()), title);
    }
    for resource in resources_at(request, path, row) {
        let url = resource.url(path).unwrap();
        let title = stdlib::shown(stdlib::resource::control(
            &mut engine,
            resource.record(path),
        ));
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
    if wants_lookup(doc, row) {
        push(
            Action::Refresh {
                document: Some(uri),
            },
            titled(&mut engine, "refresh", "lookups"),
        );
    }
    result
}
/// Whether a row reads a lookup, and so offers to refresh lookups.
fn wants_lookup(doc: &Document, row: usize) -> bool {
    let line = doc.line(row);
    ["rate(", "to(", "forecast(", "forecast_range(", "quote("]
        .iter()
        .any(|call| line.contains(call))
        || doc.days.iter().any(|d| d.line == row && d.places.is_some())
}
pub(crate) fn lenses(
    request: &lang::eval::RequestContext<'_>,
    path: &Path,
    capabilities: Capabilities,
) -> Vec<CodeLens> {
    let ws = request.workspace();

    let Some(doc) = ws.documents().get(path) else {
        return vec![];
    };
    // Every row the editor's own controls can be on; the modules add theirs.
    let rows: BTreeSet<_> = doc
        .definitions
        .iter()
        .map(|d| d.named.span.line)
        .chain(doc.tasks.iter().map(|t| t.line))
        .chain(doc.references.iter().map(|r| r.span.line))
        .chain(doc.links.iter().map(|l| l.span.line))
        .chain((0..doc.text.lines().count()).filter(|row| wants_lookup(doc, *row)))
        .collect();
    providers::row_controls(request, path, rows, TaskToggle::Command, capabilities)
        .into_iter()
        .map(|(row, command)| CodeLens {
            range: Range::new(Position::new(row as u32, 0), Position::new(row as u32, 0)),
            command: Some(command),
            data: None,
        })
        .collect()
}
