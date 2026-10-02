//! Catalog values are plain language values. These helpers keep the three
//! conventions the query API relies on: host objects enter as the records they
//! describe, positions and ranges read as counts, and lists and records display
//! as JSON.
use lang::eval::engine::{HostObject, Value, value_json};
use lsp_types::{Position, Range};
use std::sync::Arc;

pub(crate) fn text(value: impl Into<String>) -> Value {
    Value::Text(value.into())
}
/// An editor position, the shape `anchor` and every `range` end take.
pub(crate) fn position(position: Position) -> Value {
    Value::record(
        [
            ("line".into(), Value::Count(position.line as usize)),
            (
                "character".into(),
                Value::Count(position.character as usize),
            ),
        ]
        .into(),
    )
}
pub(crate) fn range(range: Range) -> Value {
    Value::record(
        [
            ("start".into(), position(range.start)),
            ("end".into(), position(range.end)),
        ]
        .into(),
    )
}
pub(crate) fn from_json(value: serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(v) => Value::Bool(v),
        serde_json::Value::Number(v) => v
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .map(Value::Count)
            .unwrap_or_else(|| Value::Number(v.as_f64().unwrap())),
        serde_json::Value::String(v) => Value::Text(v),
        serde_json::Value::Array(v) => Value::list(v.into_iter().map(from_json).collect()),
        serde_json::Value::Object(v) => {
            Value::record(v.into_iter().map(|(k, v)| (k, from_json(v))).collect())
        }
    }
}
/// Host objects describe themselves as plain language values; a namespace
/// has no such shape and stays the value it is. A list or record with no host
/// object anywhere inside is already plain, so it is kept, shared, as it is.
pub(crate) fn query_value(value: Value) -> Value {
    if let Some(record) = value.host().and_then(HostObject::query) {
        return query_value(record);
    }
    match value {
        Value::List(values) if values.iter().any(holds_host) => Value::list(
            Arc::unwrap_or_clone(values)
                .into_inner()
                .into_iter()
                .map(query_value)
                .collect(),
        ),
        Value::Record(fields) if fields.values().any(holds_host) => Value::record(
            Arc::unwrap_or_clone(fields)
                .into_inner()
                .into_iter()
                .map(|(k, v)| (k, query_value(v)))
                .collect(),
        ),
        plain => plain,
    }
}
/// Whether a value is, or holds somewhere inside, a host object.
fn holds_host(value: &Value) -> bool {
    value.host().is_some()
        || match value {
            Value::List(values) => values.iter().any(holds_host),
            Value::Record(fields) => fields.values().any(holds_host),
            _ => false,
        }
}
/// A row as a command line prints it: scalars as a note would show them,
/// `null` as itself, and lists and records as JSON.
pub fn display(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::List(_) | Value::Record(_) => value_json(value).to_string(),
        scalar => scalar.display(),
    }
}
/// A query's row as one line of `xmd`'s text output: a record as tab-separated
/// `field=value` pairs, its `source` as `path:line`, and anything else as
/// [`display`] shows it. The browser's terminal prints the same lines.
pub fn display_row(value: &Value) -> String {
    let Value::Record(fields) = value else {
        return display(value);
    };
    fields
        .iter()
        .map(|(name, value)| {
            let text = if name == "source"
                && let Value::Record(source) = value
                && let Some(Value::Text(path)) = source.get("path")
                && let Some(line) = source.get("line")
            {
                format!("{path}:{}", display(line))
            } else {
                display(value)
            };
            format!("{name}={text}")
        })
        .collect::<Vec<_>>()
        .join("\t")
}
