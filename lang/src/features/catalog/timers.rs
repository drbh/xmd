//! Timer records: every definition and reference that evaluates to a timer.
use super::{
    QueryValue, RecordKind,
    record::{Base, Fields, Record, TimerOrigin, projected},
};
use crate::{
    document::{Document, Span},
    engine::{Engine, Value},
    workspace::Workspace,
};
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Debug)]
pub(super) struct TimerRecord {
    base: Base,
    name: String,
    definition: bool,
    inlay: bool,
    value: QueryValue,
    origin: Option<TimerOrigin>,
}
impl TimerRecord {
    pub(super) const FIELDS: [&'static str; 5] = ["name", "definition", "inlay", "value", "origin"];
}
impl Fields for TimerRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(projected(
            TimerRecord::FIELDS,
            [
                QueryValue::text(self.name),
                QueryValue::boolean(self.definition),
                QueryValue::boolean(self.inlay),
                self.value,
                self.origin
                    .map(|o| QueryValue::Object(o.fields()))
                    .unwrap_or(QueryValue::Null),
            ],
        ));
        fields
    }
}

pub(super) fn timers(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    engine: &mut Engine<'_>,
    records: &mut Vec<Record>,
) {
    let occurrences = doc
        .definitions
        .iter()
        .map(|d| {
            (
                d.named.name.as_str(),
                d.named.span.line,
                d.end.range(&doc.text).start,
                true,
                d.expression,
            )
        })
        .chain(doc.references.iter().map(|r| {
            let end = if r.bracket {
                r.end() + doc.line(r.span.line)[r.end()..].find(']').unwrap_or(0) + 1
            } else {
                r.end()
            };
            (
                &*r.name,
                r.span.line,
                Span::new(r.span.line, end, end).range(&doc.text).end,
                false,
                r.bracket && r.property.is_none(),
            )
        }));
    for (name, line, anchor, definition, inlay) in occurrences {
        let Ok(Value::Timer(timer)) = engine.named(path, name) else {
            continue;
        };
        let mut base = Base::new(ws, path, line, RecordKind::Timer, name);
        base.anchor = anchor;
        records.push(Record::typed(
            path,
            TimerRecord {
                base,
                name: name.into(),
                definition,
                inlay,
                value: QueryValue::from_value(timer.record()),
                origin: timer.origin.as_ref().map(|origin| TimerOrigin {
                    document: crate::paths::file_url(&origin.path).unwrap().into(),
                    name: ws.named(origin).name.clone(),
                }),
            },
        ));
    }
}
