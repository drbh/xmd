//! Mention records: every place a note names something it reads, in the
//! order the note is read: a reference to a name, bracketed (`[focus]`,
//! `[focus.remaining]`) or inside an expression or an attribute's value. A
//! mention carries no value: a module that wants one reads the name's
//! definition among the note's `values`, so a feature that shows something
//! wherever its values are named (the timers) builds its records from both.
use super::{
    RecordKind,
    record::{Base, Record},
};
use lang::common::Span;
use lang::eval::Workspace;
use lang::eval::record;
use lang::model::Document;
use std::path::Path;

record! {
    #[derive(Clone, Debug)]
    pub(super) struct MentionRecord {
        ..base: Base,
        name: String,
        /// Whether the reference is written in brackets.
        bracket: bool,
        /// The property a bracketed reference reads, as in `[focus.remaining]`.
        property: Option<String>,
    }
}

pub(super) fn mentions(ws: &Workspace, path: &Path, doc: &Document, records: &mut Vec<Record>) {
    for reference in &doc.references {
        let end = if reference.bracket {
            doc.reference_close(reference)
        } else {
            reference.end()
        };
        let line = reference.span.line;
        records.push(Record::typed(
            path,
            MentionRecord {
                base: Base::at(
                    ws,
                    path,
                    RecordKind::Mention,
                    &*reference.name,
                    doc.line_span(line),
                    Some(Span::new(line, end, end).range(doc).end),
                ),
                name: reference.name.to_string(),
                bracket: reference.bracket,
                property: reference.property.clone(),
            },
        ));
    }
}
