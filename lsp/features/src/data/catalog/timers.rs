//! Timer records: every definition and reference that evaluates to a timer.
use super::value as q;
use super::{
    RecordKind,
    record::{Base, Record, TimerOrigin},
};
use common::Span;
use eval::Workspace;
use eval::engine::{Engine, Value};
use eval::record;
use model::Document;
use std::path::Path;

record! {
    #[derive(Clone, Debug)]
    pub(super) struct TimerRecord {
        ..base: Base,
        name: String,
        definition: bool,
        inlay: bool,
        value: Value,
        origin: Option<TimerOrigin>,
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
        base.set_anchor(anchor);
        records.push(Record::typed(
            path,
            TimerRecord {
                base,
                name: name.into(),
                definition,
                inlay,
                value: q::query_value(timer.record()),
                origin: timer.origin.as_ref().map(|origin| TimerOrigin {
                    document: common::file_url(&origin.path).unwrap().into(),
                    name: ws.named(origin).name.clone(),
                }),
            },
        ));
    }
}
