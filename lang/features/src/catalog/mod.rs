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

use eval::Workspace;
use eval::engine::Engine;
use std::path::Path;

pub(crate) use eval::Clock as QueryContext;
pub(crate) use eval::modules::Collection;
pub(crate) use record::{Record, source};
pub use value::QueryValue;

/// The `kind` field every catalog record carries. Queries and .wtf modules
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

/// The field names records in this collection carry, taken from the record
/// structs' own `FIELDS` lists rather than written out again. Kept apart from
/// `Collection` itself (in `evaluate::modules`), since it reads the record
/// types feature modules define below.
pub(crate) trait CollectionFields {
    fn fields(self) -> Vec<&'static str>;
}
impl CollectionFields for Collection {
    fn fields(self) -> Vec<&'static str> {
        let base = record::Base::FIELDS.to_vec();
        let with = |extra: &[&'static str]| {
            let mut fields = base.clone();
            fields.extend_from_slice(extra);
            fields
        };
        let scheduled = |extra: &[&'static str]| {
            let mut fields = base.clone();
            fields.extend_from_slice(&record::Scheduling::FIELDS);
            fields.extend_from_slice(extra);
            fields
        };
        let expression = |extra: &[&'static str]| {
            let mut fields = base.clone();
            fields.extend_from_slice(&record::Expression::FIELDS);
            fields.extend_from_slice(extra);
            fields
        };
        match self {
            Self::Ast => crate::inspection::AST_FIELDS.to_vec(),
            Self::Days => with(&days::DayRecord::FIELDS),
            Self::Timers => with(&timers::TimerRecord::FIELDS),
            Self::Links => with(&links::LinkRecord::FIELDS),
            // `entries` is tasks, events and stops together, so it answers for
            // the widest of the three.
            Self::Tasks | Self::Entries => scheduled(&tasks::TaskRecord::FIELDS),
            Self::Events | Self::Stops => scheduled(&[]),
            Self::Values | Self::Tables => with(&definitions::DefinitionRecord::FIELDS),
            Self::Plans => {
                let mut fields = with(&definitions::DefinitionRecord::FIELDS);
                fields.extend_from_slice(&definitions::DefinitionRecord::SOLUTION);
                fields
            }
            Self::Rows => with(&definitions::RowRecord::FIELDS),
            Self::Decisions => with(&definitions::DecisionRecord::FIELDS),
            Self::Resources => with(&links::ResourceRecord::FIELDS),
            Self::Diagnostics => with(&diagnostics::DiagnosticRecord::FIELDS),
            Self::Notes => with(&notes::NoteRecord::FIELDS),
            Self::Sections => with(&sections::SectionRecord::FIELDS),
            Self::Calculations => expression(&expressions::CalculationRecord::FIELDS),
            Self::References => expression(&expressions::ReferenceRecord::FIELDS),
            Self::Cells => expression(&expressions::CellRecord::FIELDS),
        }
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
                records.extend(crate::inspection::ast(ws, path));
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
