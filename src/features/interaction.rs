use crate::{
    actions,
    engine::{Engine, Value},
    resources::Resource,
    workspace::Workspace,
};
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
    let Some(doc) = ws.documents.get(path) else {
        return vec![];
    };
    let mut engine = Engine::at(ws, now);
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
    let Some(doc) = ws.documents.get(path) else {
        return vec![];
    };
    let uri = crate::paths::file_url(path).unwrap();
    let mut result = vec![];
    let mut seen = BTreeSet::new();
    let mut engine = Engine::at(ws, now);
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
        if let Ok(Value::Timer(t)) = engine.named(path, name)
            && let Some(origin) = &t.origin
            && seen.insert(origin.clone())
        {
            for action in t.actions() {
                let name = &ws.named(origin).name;
                result.push(Command {
                    title: format!(
                        "{}{} timer '{name}'",
                        action[..1].to_uppercase(),
                        &action[1..]
                    ),
                    command: "wtf.timer".into(),
                    arguments: Some(vec![
                        serde_json::json!(crate::paths::file_url(&origin.path).unwrap()),
                        serde_json::json!(name),
                        serde_json::json!(action),
                    ]),
                });
            }
        }
    }
    if include_task
        && let Some((index, task)) = doc.tasks.iter().enumerate().find(|(_, t)| t.line == row)
        && actions::toggle_task(ws, path, index, now.date_naive()).is_ok()
    {
        let title = if task.attributes.contains_key("every") {
            "Complete occurrence and schedule next"
        } else if engine.task_done(path, index) {
            "Reopen task"
        } else {
            "Complete task"
        };
        result.push(Command {
            title: title.into(),
            command: "wtf.task".into(),
            arguments: Some(vec![
                serde_json::json!(uri),
                serde_json::json!(row),
                serde_json::json!(doc.line(row)),
            ]),
        });
    }
    for resource in resources_at(ws, path, row, now) {
        let url = resource.url(path).unwrap();
        let args = vec![
            serde_json::json!(uri),
            serde_json::json!(row),
            serde_json::json!(doc.line(row)),
            serde_json::json!(url),
        ];
        result.push(Command {
            title: format!(
                "Open {}",
                if resource.is_image() {
                    "image"
                } else if resource.target.starts_with("geo:") {
                    "map"
                } else {
                    "resource"
                }
            ),
            command: "wtf.openResource".into(),
            arguments: Some(args.clone()),
        });
        if let Some(request) = crate::link_features::BUILTINS.refresh_request(&resource.target) {
            result.push(Command {
                title: request.title.into(),
                command: "wtf.refreshResource".into(),
                arguments: Some(args),
            });
        }
    }
    let line = doc.line(row);
    let wants_lookup = ["rate(", "to(", "forecast(", "quote("]
        .iter()
        .any(|call| line.contains(call))
        || doc.days.iter().any(|d| d.line == row && d.places.is_some());
    if wants_lookup {
        result.push(Command {
            title: "Refresh lookups".into(),
            command: "wtf.refresh".into(),
            arguments: Some(vec![serde_json::json!(uri)]),
        });
    }
    result
}
pub fn lenses(ws: &Workspace, path: &Path, now: DateTime<FixedOffset>) -> Vec<CodeLens> {
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
        .collect();
    rows.into_iter()
        .flat_map(|row| {
            row_commands(ws, path, row, now, true)
                .into_iter()
                .map(move |command| CodeLens {
                    range: Range::new(Position::new(row as u32, 0), Position::new(row as u32, 0)),
                    command: Some(command),
                    data: None,
                })
        })
        .collect()
}
