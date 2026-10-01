//! Attributed records: one per line that writes an attribute a module
//! declares, live there, with each such attribute evaluated in the note's
//! scope as its declaration says. This is how a module reads what a note
//! wrote in an attribute without evaluating note code itself: the engine
//! does it here, with its memo, cycle checks and relative dates, and the
//! module gets values. What a line with one of them means is the declaring
//! module's to say.
use super::value as q;
use super::{
    RecordKind,
    record::{Base, Record, SourceRef},
};
use lang::document::Document;
use lang::eval::engine::{Engine, Value};
use lang::eval::{Clock, ToValue, Workspace, record};
use lang::syntax::AttributeValue as Holds;
use std::{collections::BTreeMap, path::Path};

record! {
    #[derive(Clone, Debug)]
    pub(super) struct AttributedRecord {
        ..base: Base,
        block: String,
        task: bool,
        range: Value,
        attributes: BTreeMap<String, Value>,
    }
}

record! {
    /// One declared attribute of a line, as written and as evaluated.
    #[derive(Clone, Debug)]
    struct AttributeValue {
        text: String,
        value: Value,
        date: Value,
        error: Value,
        range: Value,
        value_range: Value,
    }
}

pub(super) fn attributed(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    engine: &mut Engine<'_>,
    records: &mut Vec<Record>,
) {
    let clock = Clock::new(engine.now());
    for line in doc.claimed() {
        let item = doc
            .tasks()
            .binary_search_by_key(&line.line, |t| t.line)
            .ok()
            .filter(|_| line.checkbox);
        let mut attributes = BTreeMap::new();
        for (declared, attribute) in doc.live_attributes(line) {
            let value =
                engine
                    .attribute(path, declared, item, attribute)
                    .map(|value| match declared.value {
                        Holds::Dependencies => waiting(ws, path, value),
                        _ => value,
                    });
            attributes.insert(
                declared.key.clone(),
                AttributeValue {
                    text: attribute.value.clone(),
                    date: value.as_ref().ok().and_then(|v| clock.date(v)).to_value(),
                    error: value
                        .as_ref()
                        .err()
                        .map_or(Value::Null, |e| q::text(e.to_string())),
                    value: value.map_or(Value::Null, q::query_value),
                    range: q::range(attribute.span.range(doc)),
                    value_range: q::range(attribute.value_span.range(doc)),
                }
                .to_value(),
            );
        }
        if attributes.is_empty() {
            continue;
        }
        records.push(Record::typed(
            path,
            AttributedRecord {
                base: Base::line(ws, path, RecordKind::Attributed, &line.title, line.line),
                block: <&str>::from(line.on).into(),
                task: line.checkbox,
                range: q::range(analysis::line_range(doc, line.line)),
                attributes,
            },
        ));
    }
}

record! {
    /// A condition a dependencies attribute waits on: `text` as written,
    /// and when it names something, that name as declared and where.
    #[derive(Clone, Debug)]
    struct Waiting {
        text: String,
        name: String,
        source: Option<SourceRef>,
    }
}

/// The unmet conditions the engine names, each with where it is declared.
fn waiting(ws: &Workspace, path: &Path, names: Value) -> Value {
    let Value::List(names) = names else {
        return names;
    };
    let waiting = |text: &str| match ws.resolve(path, text).ok() {
        Some(symbol) => {
            let named = ws.named(&symbol);
            Waiting {
                text: text.into(),
                name: named.name.clone(),
                source: Some(SourceRef::new(ws, &symbol.path, named.span)),
            }
        }
        None => Waiting {
            text: text.into(),
            name: text.into(),
            source: None,
        },
    };
    Value::list(
        names
            .iter()
            .map(|name| match name {
                Value::Text(text) => waiting(text).to_value(),
                other => other.clone(),
            })
            .collect(),
    )
}
