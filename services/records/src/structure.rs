//! The records of a note's plain structure, read straight off the parsed
//! document: the note itself, its sections and its mentions.
use super::{
    RecordKind,
    record::{Base, Record},
};
use lang::common::Span;
use lang::document::Document;
use lang::eval::Workspace;
use lang::eval::record;
use std::path::Path;

record! {
    /// A note record: one per document, carrying its whole text.
    #[derive(Clone, Debug)]
    pub(super) struct NoteRecord {
        ..base: Base,
        text: String,
    }
}

pub(super) fn notes(ws: &Workspace, path: &Path, doc: &Document, records: &mut Vec<Record>) {
    records.push(Record::typed(
        path,
        NoteRecord {
            base: Base::line(
                ws,
                path,
                RecordKind::Note,
                path.file_name().unwrap_or_default().to_string_lossy(),
                0,
            ),
            text: doc.text().into(),
        },
    ));
}

record! {
    /// A section record: one per heading, with the span of lines it covers.
    #[derive(Clone, Debug)]
    pub(super) struct SectionRecord {
        ..base: Base,
        end_line: usize,
        level: usize,
    }
}

pub(super) fn sections(ws: &Workspace, path: &Path, doc: &Document, records: &mut Vec<Record>) {
    for section in doc.sections() {
        records.push(Record::typed(
            path,
            SectionRecord {
                base: Base::line(ws, path, RecordKind::Section, &section.title, section.line),
                end_line: section.end_line,
                level: section.level,
            },
        ));
    }
}

record! {
    /// A mention record: every place a note names something it reads, in the
    /// order the note is read: a reference to a name, bracketed (`[focus]`,
    /// `[focus.remaining]`) or inside an expression or an attribute's value. A
    /// mention carries no value: a module that wants one reads the name's
    /// definition among the note's `values`, so a feature that shows something
    /// wherever its values are named (the timers) builds its records from both.
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
    for reference in doc.references() {
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
