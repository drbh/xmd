//! Section records: one per heading, with the span of lines it covers.
use super::{
    QueryValue, RecordKind,
    record::{Base, Fields, Record, entries},
};
use crate::{document::Document, workspace::Workspace};
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Debug)]
struct SectionRecord {
    base: Base,
    end_line: usize,
    level: usize,
}
impl Fields for SectionRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(entries([
            ("end_line", QueryValue::count(self.end_line)),
            ("level", QueryValue::count(self.level)),
        ]));
        fields
    }
}

pub(super) fn sections(ws: &Workspace, path: &Path, doc: &Document, records: &mut Vec<Record>) {
    for section in &doc.sections {
        records.push(Record::typed(
            path,
            SectionRecord {
                base: Base::new(ws, path, section.line, RecordKind::Section, &section.title),
                end_line: section.end_line,
                level: section.level,
            },
        ));
    }
}
