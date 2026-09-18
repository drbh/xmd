use crate::commands::{Action, Capabilities, RowTarget};
use crate::{actions, engine::Value, resources::Resource, workspace::Workspace};
use chrono::{DateTime, FixedOffset};
use lsp_types::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub fn resources_at(
    ws: &Workspace,
    path: &Path,
    row: usize,
    now: DateTime<FixedOffset>,
) -> Vec<Resource> {
    resources_at_in(&crate::RequestContext::new(ws, now), path, row)
}
pub fn resources_at_in(
    request: &crate::RequestContext<'_>,
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
pub fn row_commands(
    ws: &Workspace,
    path: &Path,
    row: usize,
    now: DateTime<FixedOffset>,
    include_task: bool,
) -> Vec<Command> {
    row_commands_in(
        &crate::RequestContext::new(ws, now),
        path,
        row,
        include_task,
    )
}
pub fn row_commands_in(
    request: &crate::RequestContext<'_>,
    path: &Path,
    row: usize,
    include_task: bool,
) -> Vec<Command> {
    row_commands_for(request, path, row, include_task, Capabilities::NATIVE)
}
pub fn row_commands_for(
    request: &crate::RequestContext<'_>,
    path: &Path,
    row: usize,
    include_task: bool,
    capabilities: Capabilities,
) -> Vec<Command> {
    let ws = request.workspace();
    let Some(doc) = ws.documents.get(path) else {
        return vec![];
    };
    let uri = crate::paths::file_url(path).unwrap();
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
    let engine = request.engine();
    if include_task
        && let Some((index, task)) = doc.tasks.iter().enumerate().find(|(_, t)| t.line == row)
        && actions::toggle_task_in(request, path, index).is_ok()
    {
        let title = if task.attributes.contains_key("every") {
            "Complete occurrence and schedule next"
        } else if engine.task_done(path, index) {
            "Reopen task"
        } else {
            "Complete task"
        };
        push(Action::ToggleTask(target.clone()), title.into());
    }
    for resource in resources_at_in(request, path, row) {
        let url = resource.url(path).unwrap();
        let kind = if resource.is_image() {
            "image"
        } else if resource.target.starts_with("geo:") {
            "map"
        } else {
            "resource"
        };
        push(
            Action::OpenResource {
                target: target.clone(),
                url: url.clone(),
            },
            format!("Open {kind}"),
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
    let wants_lookup = ["rate(", "to(", "forecast(", "quote("]
        .iter()
        .any(|call| line.contains(call))
        || doc.days.iter().any(|d| d.line == row && d.places.is_some());
    if wants_lookup {
        push(
            Action::Refresh {
                document: Some(uri),
            },
            "Refresh lookups".into(),
        );
    }
    result.extend(super::plugin_inlays::commands(
        request,
        path,
        row,
        capabilities,
    ));
    result
}
pub fn lenses(ws: &Workspace, path: &Path, now: DateTime<FixedOffset>) -> Vec<CodeLens> {
    lenses_in(&crate::RequestContext::new(ws, now), path)
}
pub fn lenses_in(request: &crate::RequestContext<'_>, path: &Path) -> Vec<CodeLens> {
    lenses_for(request, path, Capabilities::NATIVE)
}
pub fn lenses_for(
    request: &crate::RequestContext<'_>,
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
                .plugins
                .active()
                .any(|m| m.kind == "inlay" && m.has("actions"))
            {
                0..doc.text.lines().count()
            } else {
                0..0
            },
        )
        .collect();
    rows.into_iter()
        .flat_map(|row| {
            row_commands_for(request, path, row, true, capabilities)
                .into_iter()
                .map(move |command| CodeLens {
                    range: Range::new(Position::new(row as u32, 0), Position::new(row as u32, 0)),
                    command: Some(command),
                    data: None,
                })
        })
        .collect()
}
