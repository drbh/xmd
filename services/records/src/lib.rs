//! Typed, host-independent workspace records, and the `xmd query` language
//! over them. Reading records never performs I/O. Exposes its interface from
//! the root, as one list of names.
//!
//! A collection is a named set of records — checklist items, mentions,
//! definitions and the rest — each a struct that knows its own field names. One module per record
//! family holds the struct, its field projection and the builder that walks the
//! workspace for it (`structure` holds the plain ones: notes, sections and
//! mentions); this root names the collections and dispatches to them.
//! A collection a feature module declares is built by its `records` hook
//! (`built`), from what `context` hands every hook of a module.
//! `query` evaluates queries over the collections, and `inspection` adds the
//! syntax and dependency views that queries and feature modules read.
//!
//! Every reader takes records from `cache` ([`Records`]), which builds each
//! collection once per workspace revision and clock and keeps what its lazy
//! fields produce.
mod attributed;
mod built;
mod cache;
mod checkboxes;
mod context;
mod definitions;
mod diagnostics;
mod expressions;
mod inspection;
mod links;
mod lookups;
mod presentations;
mod query;
mod recognized;
mod record;
mod structure;
mod value;

use lang::eval::engine::Engine;
use lang::eval::{RequestContext, Workspace};
use lsp_types::Diagnostic;
use std::path::Path;

pub use built::problems as build_problems;
pub use cache::{Records, View};
pub use context::feature_context;
use lang::eval::modules::Collection;
pub use lookups::lookups;
pub use presentations::presentations;
pub use query::{NoteFiles, Query, QueryResult, execute};
use record::Record;
use record::SourceRef;
pub use value::{display, display_row};

/// Where the `diagnostics` collection comes from, chosen by the caller.
/// Queries see what an editor shows, feature modules' own diagnostics
/// included; a feature module that reads the collection sees only the
/// language's.
pub type DiagnosticSource = fn(&RequestContext<'_>, &Path) -> Vec<Diagnostic>;

/// The `kind` field every record carries. Queries and .xmd modules
/// match on these names, so they are part of the workspace's data contract.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, strum::Display, strum::IntoStaticStr,
)]
#[strum(serialize_all = "snake_case")]
enum RecordKind {
    Checkbox,
    Mention,
    Link,
    Section,
    Calculation,
    Reference,
    Cell,
    Note,
    Value,
    Table,
    Row,
    Resource,
    Diagnostic,
    Recognized,
    Attributed,
}
impl lang::eval::ToValue for RecordKind {
    fn to_value(&self) -> lang::eval::engine::Value {
        value::text(<&str>::from(*self))
    }
}

/// A cache whose collections are the ones [`collect`] indexes.
impl Default for Records {
    fn default() -> Self {
        Self::new(collect)
    }
}

/// The query API and feature modules read the same semantic records. Each
/// collection is one record builder below; this match is their index.
fn collect(
    cache: &Records,
    collection: Collection,
    engine: &mut Engine<'_>,
    only: Option<&Path>,
    diagnostics: DiagnosticSource,
) -> Result<Vec<Record>, String> {
    let ws: &Workspace = engine.workspace();
    let mut records = Vec::new();
    for (path, doc) in ws.documents() {
        if only.is_some_and(|wanted| wanted != path) {
            continue;
        }
        match &collection {
            Collection::Ast => records.extend(inspection::ast(ws, path)),
            Collection::Mentions => structure::mentions(ws, path, doc, &mut records),
            Collection::Links => links::links(ws, path, doc, &mut records),
            Collection::Sections => structure::sections(ws, path, doc, &mut records),
            Collection::Calculations => {
                expressions::calculations(ws, path, doc, engine, &mut records)
            }
            Collection::References => expressions::references(ws, path, doc, engine, &mut records),
            Collection::Cells => expressions::cells(ws, path, doc, engine, &mut records),
            Collection::Notes => structure::notes(ws, path, doc, &mut records),
            Collection::Checkboxes => checkboxes::checkboxes(ws, path, doc, &mut records),
            Collection::Entries => built::entries(cache, engine, path, &mut records),
            Collection::Values | Collection::Forms | Collection::Tables | Collection::Rows => {
                definitions::definitions(ws, path, doc, &collection, engine, &mut records)?
            }
            Collection::Resources => links::resources(ws, path, doc, &mut records),
            Collection::Recognized => recognized::recognized(ws, path, doc, &mut records),
            Collection::Attributed => attributed::attributed(ws, path, doc, engine, &mut records),
            Collection::Diagnostics => {
                diagnostics::diagnostics(ws, path, engine, diagnostics, &mut records)
            }
            Collection::Declared(name) => {
                built::collection(cache, engine, path, name, &mut records)
            }
        }
    }
    Ok(records)
}
