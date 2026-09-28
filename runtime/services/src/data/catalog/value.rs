//! Catalog values are plain language values. These helpers keep the three
//! conventions the query API relies on: host objects enter as the records they
//! describe, JSON integers (positions, ranges) arrive as counts, and lists and
//! records display as JSON.
use lang::eval::engine::{HostObject, Value, value_json};

pub(crate) fn text(value: impl Into<String>) -> Value {
    Value::Text(value.into())
}
pub(crate) fn strings(values: impl IntoIterator<Item = String>) -> Value {
    Value::List(values.into_iter().map(Value::Text).collect())
}
pub(crate) fn object<const N: usize>(fields: [(&str, Value); N]) -> Value {
    Value::Record(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
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
        serde_json::Value::Array(v) => Value::List(v.into_iter().map(from_json).collect()),
        serde_json::Value::Object(v) => {
            Value::Record(v.into_iter().map(|(k, v)| (k, from_json(v))).collect())
        }
    }
}
/// Host objects describe themselves as plain language values; a namespace
/// has no such shape and stays the value it is.
pub(crate) fn query_value(value: Value) -> Value {
    if let Some(record) = value.host().and_then(HostObject::query) {
        return query_value(record);
    }
    match value {
        Value::List(values) => Value::List(values.into_iter().map(query_value).collect()),
        Value::Record(fields) => Value::Record(
            fields
                .into_iter()
                .map(|(k, v)| (k, query_value(v)))
                .collect(),
        ),
        scalar => scalar,
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
