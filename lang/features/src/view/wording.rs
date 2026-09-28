//! The words a label, hover or control title uses come from the stdlib's
//! library modules (`format`, `task`, `resource`, `today`); these ask for them.
use eval::engine::{Engine, Value};

/// Ask a library to word something, showing its error if it fails.
pub(crate) fn present(
    engine: &mut Engine<'_>,
    module: &str,
    name: &str,
    args: Vec<Value>,
) -> String {
    engine
        .call_module(module, name, args)
        .map(|v| v.display())
        .unwrap_or_else(|e| e.to_string())
}

/// `format.series` over a column or a sum's rows: a sparkline and its range,
/// or nothing when fewer than two values can be charted.
pub(crate) fn series(engine: &mut Engine<'_>, values: Vec<Value>) -> Option<String> {
    match engine.call_module("format", "series", vec![Value::List(values)]) {
        Ok(Value::Null) => None,
        Ok(chart) => Some(chart.display()),
        Err(e) => Some(e.to_string()),
    }
}

/// A glyph from `format.glyph` followed by a word, as control titles read.
pub(crate) fn titled(engine: &mut Engine<'_>, glyph: &str, word: &str) -> String {
    format!(
        "{} {word}",
        present(engine, "format", "glyph", vec![Value::Text(glyph.into())])
    )
}

/// The title of the control that completes, reopens or advances a task.
pub(crate) fn task_toggle(engine: &mut Engine<'_>, recurring: bool, done: bool) -> String {
    present(
        engine,
        "task",
        "toggle",
        vec![Value::Bool(recurring), Value::Bool(done)],
    )
}
