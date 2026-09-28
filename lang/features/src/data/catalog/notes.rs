//! Note records: one per document, carrying its whole text.
use super::{
    RecordKind,
    record::{Base, Record},
};
use eval::Workspace;
use eval::record;
use model::Document;
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
            base: Base::new(
                ws,
                path,
                0,
                RecordKind::Note,
                path.file_name().unwrap_or_default().to_string_lossy(),
            ),
            text: doc.text.clone(),
        },
    ));
}
