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
pub(crate) mod value;

use lang::eval::Workspace;
use lang::eval::engine::Engine;
use std::path::Path;

pub(crate) use lang::eval::Clock as QueryContext;
pub(crate) use lang::eval::modules::Collection;
pub(crate) use record::{Record, source};

/// The `kind` field every catalog record carries. Queries and .x.md modules
/// match on these names, so they are part of the workspace's data contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub(crate) enum RecordKind {
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
    pub(crate) fn as_str(self) -> &'static str {
        self.into()
    }
}
impl std::fmt::Display for RecordKind {
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
                records.extend(crate::data::inspection::ast(ws, path));
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
