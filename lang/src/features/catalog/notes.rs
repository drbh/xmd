//! Note records: one per document, carrying its whole text.
use super::{
    QueryValue, RecordKind,
    record::{Base, Fields, Record, projected},
};
use crate::{document::Document, workspace::Workspace};
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Debug)]
pub(super) struct NoteRecord {
    base: Base,
    text: String,
}
impl NoteRecord {
    pub(super) const FIELDS: [&'static str; 1] = ["text"];
}
impl Fields for NoteRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(projected(NoteRecord::FIELDS, [QueryValue::text(self.text)]));
        fields
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
