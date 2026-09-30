//! Link and resource records: the targets a note points at.
use super::value as q;
use super::{
    RecordKind,
    record::{Base, Record},
};
use lang::common::Span;
use lang::eval::Workspace;
use lang::eval::engine::Value;
use lang::eval::record;
use lang::model::Document;
use serde_json::json;
use std::path::Path;

record! {
    #[derive(Clone, Debug)]
    pub(super) struct LinkRecord {
        ..base: Base,
        url: String,
    }
}

record! {
    #[derive(Clone, Debug)]
    pub(super) struct ResourceRecord {
        ..base: Base,
        target: String,
        metadata: Value,
    }
}

pub(super) fn links(ws: &Workspace, path: &Path, doc: &Document, records: &mut Vec<Record>) {
    for link in &doc.links {
        let mut r = Record::typed(
            path,
            LinkRecord {
                base: Base::at(
                    ws,
                    path,
                    RecordKind::Link,
                    &link.target,
                    link.span,
                    Some(link.span.range(doc).end),
                ),
                url: link.target.clone(),
            },
        );
        r.resource = Some(lang::eval::resources::Resource {
            target: link.target.clone(),
            origin: Some(path.into()),
        });
        records.push(r);
    }
}

/// Every resource a note writes, as a link or as a definition's literal, and
/// where.
pub(super) fn targets(doc: &Document) -> impl Iterator<Item = (Span, &str)> {
    doc.links.iter().map(|l| (l.span, l.target.as_str())).chain(
        doc.definitions
            .iter()
            .filter(|d| {
                !d.expression && lang::eval::resources::Resource::parse(&d.source).is_some()
            })
            .map(|d| (d.value_span, d.source.as_str())),
    )
}

pub(super) fn resources(ws: &Workspace, path: &Path, doc: &Document, records: &mut Vec<Record>) {
    for (span, target) in targets(doc) {
        records.push(Record::typed(
            path,
            ResourceRecord {
                base: Base::at(ws, path, RecordKind::Resource, target, span, None),
                target: target.into(),
                metadata: ws
                    .cache()
                    .get(target)
                    .map(|m| q::from_json(json!(m)))
                    .unwrap_or(Value::Null),
            },
        ));
    }
}
