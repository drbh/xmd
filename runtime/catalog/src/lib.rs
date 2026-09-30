//! Typed, host-independent workspace records, and the `xmd query` language
//! over them. Reading a catalog never performs I/O. Exposes its interface from
//! the root, as one list of names.
//!
//! A catalog is a named set of records — tasks, days, timers, definitions and the
//! rest — each a struct that knows its own field names. One module per record
//! family holds the struct, its field projection and the builder that walks the
//! workspace for it; this root names the collections and dispatches to them.
//! `query` evaluates queries over the collections, and `inspection` adds the
//! syntax and dependency views that queries and feature modules read.
mod days;
mod definitions;
mod diagnostics;
mod expressions;
mod inspection;
mod links;
mod notes;
mod presentations;
mod query;
mod record;
mod sections;
mod tasks;
mod timers;
mod value;

use lang::eval::engine::Engine;
use lang::eval::{RequestContext, Workspace};
use lsp_types::Diagnostic;
use std::path::Path;

use lang::eval::modules::Collection;
pub use presentations::presentations;
pub use query::{NoteFiles, Query, QueryResult, execute};
pub use record::Record;
use record::SourceRef;
pub use tasks::hover as task_hover;
pub use value::display;

/// Where the `diagnostics` collection comes from, chosen by the caller.
/// Queries see what an editor shows, feature modules' own diagnostics
/// included; a feature module that reads the collection sees only the
/// language's.
pub type DiagnosticSource = fn(&RequestContext<'_>, &Path) -> Vec<Diagnostic>;

/// The `kind` field every catalog record carries. Queries and .xmd modules
/// match on these names, so they are part of the workspace's data contract.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, strum::Display, strum::IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
enum RecordKind {
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
impl lang::eval::ToValue for RecordKind {
    fn to_value(&self) -> lang::eval::engine::Value {
        value::text(<&str>::from(*self))
    }
}

/// The query API and feature modules read the same semantic records. Each
/// collection is one record builder below; this match is their index.
pub fn collect(
    ws: &Workspace,
    collection: Collection,
    engine: &mut Engine<'_>,
    only: Option<&Path>,
    diagnostics: DiagnosticSource,
) -> Result<Vec<Record>, String> {
    let mut records = Vec::new();
    match collection {
        // Two collections span documents instead of visiting them in turn.
        Collection::Ast => {
            for path in ws
                .documents()
                .keys()
                .filter(|p| only.is_none_or(|only| only == p.as_path()))
            {
                records.extend(inspection::ast(ws, path));
            }
            return Ok(records);
        }
        Collection::Decisions => {
            definitions::decisions(ws, engine, only, &mut records);
            return Ok(records);
        }
        _ => {}
    }
    for (path, doc) in ws.documents() {
        if only.is_some_and(|wanted| wanted != path) {
            continue;
        }
        match collection {
            Collection::Ast | Collection::Decisions => unreachable!("handled above"),
            Collection::Days => days::days(ws, path, doc, engine, &mut records),
            Collection::Timers => timers::timers(ws, path, doc, engine, &mut records),
            Collection::Links => links::links(ws, path, doc, &mut records),
            Collection::Sections => sections::sections(ws, path, doc, &mut records),
            Collection::Calculations => {
                expressions::calculations(ws, path, doc, engine, &mut records)
            }
            Collection::References => expressions::references(ws, path, doc, engine, &mut records),
            Collection::Cells => expressions::cells(ws, path, doc, engine, &mut records),
            Collection::Notes => notes::notes(ws, path, doc, &mut records),
            Collection::Tasks => tasks::tasks(ws, path, doc, engine, false, &mut records),
            Collection::Events => tasks::events(ws, path, doc, engine, &mut records),
            Collection::Stops => tasks::stops(ws, path, doc, engine, &mut records),
            Collection::Entries => {
                tasks::tasks(ws, path, doc, engine, true, &mut records);
                tasks::events(ws, path, doc, engine, &mut records);
                tasks::stops(ws, path, doc, engine, &mut records);
            }
            Collection::Values | Collection::Plans | Collection::Tables | Collection::Rows => {
                definitions::definitions(ws, path, doc, collection, engine, &mut records)?
            }
            Collection::Resources => links::resources(ws, path, doc, &mut records),
            Collection::Diagnostics => {
                diagnostics::diagnostics(ws, path, engine, diagnostics, &mut records)
            }
        }
    }
    Ok(records)
}
