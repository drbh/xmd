//! Link and resource records: the targets a note points at.
use super::{
    QueryValue, RecordKind,
    record::{Base, Fields, Record, SourceRef, projected},
};
use crate::{document::Document, workspace::Workspace};
use serde_json::json;
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Debug)]
pub(super) struct LinkRecord {
    base: Base,
    url: String,
}
impl LinkRecord {
    pub(super) const FIELDS: [&'static str; 1] = ["url"];
}
impl Fields for LinkRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(projected(LinkRecord::FIELDS, [QueryValue::text(self.url)]));
        fields
    }
}

#[derive(Clone, Debug)]
pub(super) struct ResourceRecord {
    base: Base,
    target: String,
    metadata: QueryValue,
}
impl ResourceRecord {
    pub(super) const FIELDS: [&'static str; 2] = ["target", "metadata"];
}
impl Fields for ResourceRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(projected(
            ResourceRecord::FIELDS,
            [QueryValue::text(self.target), self.metadata],
        ));
        fields
    }
}

pub(super) fn links(ws: &Workspace, path: &Path, doc: &Document, records: &mut Vec<Record>) {
    for link in &doc.links {
        let mut base = Base::new(ws, path, link.span.line, RecordKind::Link, &link.target);
        base.source = SourceRef::new(ws, path, link.span);
        base.anchor = link.span.range(&doc.text).end;
        let mut r = Record::typed(
            path,
            LinkRecord {
                base,
                url: link.target.clone(),
            },
        );
        r.resource = Some(crate::resources::Resource {
            target: link.target.clone(),
            origin: Some(path.into()),
        });
        records.push(r);
    }
}

pub(super) fn resources(ws: &Workspace, path: &Path, doc: &Document, records: &mut Vec<Record>) {
    let targets = doc.links.iter().map(|l| (l.span, l.target.as_str())).chain(
        doc.definitions
            .iter()
            .filter(|d| !d.expression && crate::resources::Resource::parse(&d.source).is_some())
            .map(|d| (d.value_span, d.source.as_str())),
    );
    for (span, target) in targets {
        let mut base = Base::new(ws, path, span.line, RecordKind::Resource, target);
        base.source = SourceRef::new(ws, path, span);
        records.push(Record::typed(
            path,
            ResourceRecord {
                base,
                target: target.into(),
                metadata: ws
                    .cache
                    .get(target)
                    .map(|m| QueryValue::from_json(json!(m)))
                    .unwrap_or(QueryValue::Null),
            },
        ));
    }
}
