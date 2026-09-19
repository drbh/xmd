//! Typed, host-independent workspace records. Reading a catalog never performs I/O.
//!
//! A catalog is a named set of records — tasks, days, timers, definitions and the
//! rest — each a struct that knows its own field names. One module per record
//! family holds the struct, its field projection and the builder that walks the
//! workspace for it; this module names the collections and dispatches to them.
mod days;
mod definitions;
mod diagnostics;
mod expressions;
mod links;
mod notes;
mod record;
mod sections;
mod tasks;
mod timers;
mod value;

use crate::{engine::Engine, workspace::Workspace};
use std::path::Path;

pub use crate::context::Clock as QueryContext;
pub(crate) use record::{Record, source};
pub use value::QueryValue;

/// The `kind` field every catalog record carries. Queries and .wtf modules
/// match on these names, so they are part of the workspace's data contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RecordKind {
    Task,
    Event,
    Stop,
    Day,
    Timer,
    Link,
    Section,
    Calculation,
    Reference,
    Cell,
    Note,
    Value,
    Plan,
    Table,
    Row,
    Decision,
    Resource,
    Diagnostic,
}
impl RecordKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Task => "task",
            Self::Event => "event",
            Self::Stop => "stop",
            Self::Day => "day",
            Self::Timer => "timer",
            Self::Link => "link",
            Self::Section => "section",
            Self::Calculation => "calculation",
            Self::Reference => "reference",
            Self::Cell => "cell",
            Self::Note => "note",
            Self::Value => "value",
            Self::Plan => "plan",
            Self::Table => "table",
            Self::Row => "row",
            Self::Decision => "decision",
            Self::Resource => "resource",
            Self::Diagnostic => "diagnostic",
        }
    }
}
impl std::fmt::Display for RecordKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A named set of workspace records. Queries bind these names, and feature
/// modules declare the ones they read in `module.inputs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Collection {
    Ast,
    Days,
    Timers,
    Links,
    Tasks,
    Events,
    Stops,
    Entries,
    Values,
    Plans,
    Decisions,
    Tables,
    Rows,
    Resources,
    Diagnostics,
    Notes,
    Sections,
    Calculations,
    References,
    Cells,
}
impl Collection {
    /// Declaration order is the order the query API lists them in its errors.
    pub const ALL: &'static [Collection] = &[
        Collection::Ast,
        Collection::Days,
        Collection::Timers,
        Collection::Links,
        Collection::Tasks,
        Collection::Events,
        Collection::Stops,
        Collection::Entries,
        Collection::Values,
        Collection::Plans,
        Collection::Decisions,
        Collection::Tables,
        Collection::Rows,
        Collection::Resources,
        Collection::Diagnostics,
        Collection::Notes,
        Collection::Sections,
        Collection::Calculations,
        Collection::References,
        Collection::Cells,
    ];
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ast => "ast",
            Self::Days => "days",
            Self::Timers => "timers",
            Self::Links => "links",
            Self::Tasks => "tasks",
            Self::Events => "events",
            Self::Stops => "stops",
            Self::Entries => "entries",
            Self::Values => "values",
            Self::Plans => "plans",
            Self::Decisions => "decisions",
            Self::Tables => "tables",
            Self::Rows => "rows",
            Self::Resources => "resources",
            Self::Diagnostics => "diagnostics",
            Self::Notes => "notes",
            Self::Sections => "sections",
            Self::Calculations => "calculations",
            Self::References => "references",
            Self::Cells => "cells",
        }
    }
    /// Every collection name, in declaration order, for error messages.
    pub fn names() -> String {
        Self::ALL
            .iter()
            .map(|c| c.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}
impl std::str::FromStr for Collection {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, String> {
        Self::ALL
            .iter()
            .copied()
            .find(|c| c.as_str() == value)
            .ok_or_else(|| format!("Unknown input collection: {value}"))
    }
}
impl std::fmt::Display for Collection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The query API and feature modules read the same semantic records. Each
/// collection is one record builder below; this match is their index.
pub(crate) fn collect_document(
    ws: &Workspace,
    collection: Collection,
    ctx: QueryContext,
    engine: &mut Engine<'_>,
    only: Option<&Path>,
    module_diagnostics: bool,
) -> Result<Vec<Record>, String> {
    let mut records = Vec::new();
    match collection {
        // Two collections span documents instead of visiting them in turn.
        Collection::Ast => {
            for path in ws
                .documents
                .keys()
                .filter(|p| only.is_none_or(|only| only == p.as_path()))
            {
                records.extend(crate::features::inspection::ast(ws, path));
            }
            return Ok(records);
        }
        Collection::Decisions => {
            definitions::decisions(ws, engine, only, &mut records);
            return Ok(records);
        }
        _ => {}
    }
    for (path, doc) in &ws.documents {
        if only.is_some_and(|wanted| wanted != path) {
            continue;
        }
        match collection {
            Collection::Ast | Collection::Decisions => unreachable!("handled above"),
            Collection::Days => days::days(ws, path, doc, engine, &mut records)?,
            Collection::Timers => timers::timers(ws, path, doc, engine, &mut records),
            Collection::Links => links::links(ws, path, doc, &mut records),
            Collection::Sections => sections::sections(ws, path, doc, &mut records),
            Collection::Calculations => {
                expressions::calculations(ws, path, doc, engine, &mut records)
            }
            Collection::References => expressions::references(ws, path, doc, engine, &mut records),
            Collection::Cells => expressions::cells(ws, path, doc, engine, &mut records),
            Collection::Notes => notes::notes(ws, path, doc, &mut records),
            Collection::Tasks => tasks::tasks(ws, path, doc, ctx, engine, false, &mut records),
            Collection::Events => tasks::events(ws, path, doc, ctx, engine, &mut records),
            Collection::Stops => tasks::stops(ws, path, doc, ctx, &mut records),
            Collection::Entries => {
                tasks::tasks(ws, path, doc, ctx, engine, true, &mut records);
                tasks::events(ws, path, doc, ctx, engine, &mut records);
                tasks::stops(ws, path, doc, ctx, &mut records);
            }
            Collection::Values | Collection::Plans | Collection::Tables | Collection::Rows => {
                definitions::definitions(ws, path, doc, collection, engine, &mut records)?
            }
            Collection::Resources => links::resources(ws, path, doc, &mut records),
            Collection::Diagnostics => {
                diagnostics::diagnostics(ws, path, engine, module_diagnostics, &mut records)
            }
        }
    }
    Ok(records)
}
