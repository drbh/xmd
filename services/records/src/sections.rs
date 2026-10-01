//! Section records: one per heading, with the span of lines it covers.
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
                base: Base::line(ws, path, RecordKind::Section, &section.title, section.line),
                end_line: section.end_line,
                level: section.level,
            },
        ));
    }
}
