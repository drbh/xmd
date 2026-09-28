//! A value as compact JSON: what `debug` shows, and what the catalog's
//! `QueryValue` falls back to for a scalar. A host object describes itself as
//! a plain record first; everything else keeps its own shape.
use super::{HostObject, Value};
use serde_json::json;

pub fn value_json(value: &Value) -> serde_json::Value {
    if let Some(record) = value.host().and_then(HostObject::query) {
        return value_json(&record);
    }
    match value {
        Value::Null => serde_json::Value::Null,
        Value::List(values) => serde_json::Value::Array(values.iter().map(value_json).collect()),
        Value::Record(fields) => serde_json::Value::Object(
            fields
                .iter()
                .map(|(k, v)| (k.clone(), value_json(v)))
                .collect(),
        ),
        Value::Function(_) => json!({"type":"function"}),
        Value::Namespace(path) => json!({"type":"namespace", "path":path.path()}),
        Value::Number(n) => number(*n, 10),
        Value::Count(n) => json!(n),
        Value::Text(s) => json!(s),
        Value::Code(code) => json!(code.as_str()),
        Value::Bool(b) => json!(b),
        Value::Money(amount, currency) => {
            json!({"type":"money","amount":number(*amount, 2),"currency":currency.as_str()})
        }
        Value::Duration(seconds) => json!({"type":"duration","seconds":seconds}),
        Value::Date(d) => json!({"type":"date","value":d.to_string()}),
        Value::DateTime(d) => json!({"type":"datetime","value":d.to_rfc3339()}),
        Value::Ratio(n) => json!({"type":"ratio","value":number(*n, 10)}),
        // Every other kind (Resource, Timer, Table, Plan, Forecast, task
        // lists, ...) implements `HostObject::query` and is handled above.
        _ => serde_json::Value::Null,
    }
}

/// A number as scripts expect to read it: rounded to `decimals` places so
/// binary floating point noise (`23.799999999999997`) never reaches the JSON,
/// and written as an integer when it is one (`1904`, not `1904.0`).
fn number(n: f64, decimals: i32) -> serde_json::Value {
    if !n.is_finite() {
        return json!(n);
    }
    let scale = 10f64.powi(decimals);
    let rounded = (n * scale).round() / scale;
    if rounded.fract() == 0.0 && rounded.abs() < 9007199254740992.0 {
        json!(rounded as i64)
    } else {
        json!(rounded)
    }
}
