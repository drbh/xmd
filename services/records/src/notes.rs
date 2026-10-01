//! Note records: one per document, carrying its whole text.
use super::{
    RecordKind,
    record::{Base, Record},
};
use lang::document::Document;
use lang::eval::Workspace;
use lang::eval::record;
use std::path::Path;

record! {
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
            text: doc.text.clone(),
        },
    ));
}
