//! The lookups a note reads, found by evaluating it: what `xmd refresh`
//! fetches, and the rows the ⟳ lookups lens is offered on. Which calls read a
//! lookup is the evaluator's record of every `cached` read, so any module's
//! lookup is found without naming its functions.
use crate::Records;
use lang::eval::engine::Engine;
use lang::eval::lookups::LookupKey;
use std::path::Path;

/// Every lookup the note at `path` reads: its definitions, calculations and
/// expression attributes, evaluated, and the records modules build for it.
/// Each comes with the row of the note's text it was read for, or `None` when
/// it was read for other text, such as another note's definition this one
/// uses, which offers its own refresh.
pub fn lookups(
    records: &Records,
    engine: &mut Engine<'_>,
    path: &Path,
) -> Vec<(Option<usize>, LookupKey)> {
    let ws = engine.workspace();
    let Some(doc) = ws.documents().get(path) else {
        return vec![];
    };
    let start = engine.reads().len();
    // A failure is the note's diagnostic to report; what it read is still
    // wanted.
    for symbol in ws.symbols_in(path) {
        let _ = engine.symbol(&symbol);
    }
    for calculation in &doc.calculations {
        let _ = engine.eval_at(path, &calculation.source, calculation.span);
    }
    // Only an expression can ask for a lookup.
    for (key, attr) in doc.claimed_attributes() {
        if doc.attribute_value(key).is_some_and(|v| v.is_expression()) {
            let _ = engine.eval(path, &attr.value);
        }
    }
    let mut read: Vec<_> = engine.reads()[start..]
        .iter()
        .map(|read| {
            let row = read
                .at
                .as_ref()
                .filter(|(at, _)| **at == *path)
                .map(|(_, span)| span.line);
            (row, read.key.clone())
        })
        .collect();
    // A record a module built that asks for a lookup reads it on its line.
    read.extend(
        crate::built::wanted(records, engine, path)
            .into_iter()
            .map(|(line, key)| (Some(line), key)),
    );
    read
}
