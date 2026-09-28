//! Section records: one per heading, with the span of lines it covers.
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
    pub(super) struct SectionRecord {
        ..base: Base,
        end_line: usize,
        level: usize,
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
