//! Timer records: every definition and reference that evaluates to a timer.
use super::value as q;
use super::{
    RecordKind,
    record::{Base, Record},
};
use lang::common::Span;
use lang::eval::Workspace;
use lang::eval::engine::{Engine, Value};
use lang::eval::record;
use lang::eval::timers::Timer;
use lang::model::Document;
use std::path::Path;

record! {
    /// Where a running timer was started, when that is another note's definition.
    #[derive(Clone, Debug)]
    struct TimerOrigin {
        document: String,
        name: String,
    }
}

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
                d.end.range(doc).start,
                true,
                d.expression,
            )
        })
        .chain(doc.references.iter().map(|r| {
            let end = if r.bracket {
                doc.reference_close(r)
            } else {
                r.end()
            };
            (
                &*r.name,
                r.span.line,
                Span::new(r.span.line, end, end).range(doc).end,
                false,
                r.bracket && r.property.is_none(),
            )
        }));
    for (name, line, anchor, definition, inlay) in occurrences {
        let Ok(value) = engine.named(path, name) else {
            continue;
        };
        let Some(timer) = value.downcast::<Timer>() else {
            continue;
        };
        records.push(Record::typed(
            path,
            TimerRecord {
                base: Base::at(
                    ws,
                    path,
                    RecordKind::Timer,
                    name,
                    doc.line_span(line),
                    Some(anchor),
                ),
                name: name.into(),
                definition,
                inlay,
                value: q::query_value(timer.record()),
                origin: timer.origin.as_ref().map(|origin| TimerOrigin {
                    document: lang::common::file_url(&origin.path).unwrap().into(),
                    name: ws.named(origin).name.clone(),
                }),
            },
        ));
    }
}
