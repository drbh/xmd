//! Recognized records: one per match of a recognizer a module declares,
//! read off the note as it was parsed (`Document::recognized`), so building
//! them evaluates nothing. A recognizer that only paints makes none.
use super::value as q;
use super::{
    RecordKind,
    record::{Base, Record},
};
use lang::document::Document;
use lang::eval::Workspace;
use lang::eval::engine::Value;
use lang::eval::record;
use std::path::Path;

record! {
    #[derive(Clone, Debug)]
    pub(super) struct RecognizedRecord {
        ..base: Base,
        recognizer: String,
        module: String,
        text: String,
        range: Value,
        groups: Value,
        parent: Option<usize>,
        end_line: usize,
    }
}

pub(super) fn recognized(ws: &Workspace, path: &Path, doc: &Document, records: &mut Vec<Record>) {
    for found in doc.recognized().iter().filter(|found| found.rule.record) {
        let text = found.span.source(doc);
        let range = found.span.range(doc);
        let groups = found
            .groups
            .iter()
            .map(|group| {
                let mut fields = std::collections::BTreeMap::from([
                    ("text".into(), q::text(group.span.source(doc))),
                    ("range".into(), q::range(group.span.range(doc))),
                ]);
                if !found.rule.terms(&group.name).is_empty() {
                    let term = group.term.as_deref().map_or(Value::Null, q::text);
                    fields.insert("term".into(), term);
                }
                (group.name.clone(), Value::record(fields))
            })
            .collect();
        records.push(Record::typed(
            path,
            RecognizedRecord {
                base: Base::at(
                    ws,
                    path,
                    RecordKind::Recognized,
                    text,
                    found.span,
                    Some(range.end),
                ),
                recognizer: found.rule.name.clone(),
                module: found.rule.module.clone(),
                text: text.into(),
                range: q::range(range),
                groups: Value::record(groups),
                parent: found.parent.map(|p| doc.recognized()[p].span.line),
                end_line: found.end,
            },
        ));
    }
}
