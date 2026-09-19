//! Converting between module values and the JSON and URLs hosts exchange.
use crate::engine::Value;
use crate::error::{EvalError, EvalResult};
use lsp_types::Url;

pub(super) fn text(value: &Value) -> EvalResult<String> {
    if let Value::Text(s) = value {
        Ok(s.clone())
    } else {
        Err(EvalError::Expected("text"))
    }
}
pub(super) fn strings(value: &Value) -> EvalResult<Vec<String>> {
    if let Value::List(items) = value {
        items.iter().map(text).collect()
    } else {
        Err(EvalError::Expected("a list of text"))
    }
}
pub fn record(fields: impl IntoIterator<Item = (String, Value)>) -> Value {
    Value::Record(fields.into_iter().collect())
}
pub fn from_json(value: &serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(v) => Value::Bool(*v),
        serde_json::Value::Number(v) => Value::Number(v.as_f64().unwrap_or_default()),
        serde_json::Value::String(v) => Value::Text(v.clone()),
        serde_json::Value::Array(v) => Value::List(v.iter().map(from_json).collect()),
        serde_json::Value::Object(v) => record(v.iter().map(|(k, v)| (k.clone(), from_json(v)))),
    }
}
pub fn json(value: &Value) -> EvalResult<serde_json::Value> {
    Ok(match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(v) => (*v).into(),
        Value::Number(v)
            if v.is_finite() && *v >= 0.0 && *v < u64::MAX as f64 && v.fract() == 0.0 =>
        {
            (*v as u64).into()
        }
        Value::Number(v) => serde_json::Number::from_f64(*v)
            .ok_or(EvalError::Message("Nonfinite module number".into()))?
            .into(),
        Value::Count(v) => (*v).into(),
        Value::Text(v) => v.clone().into(),
        Value::List(v) => serde_json::Value::Array(v.iter().map(json).collect::<Result<_, _>>()?),
        Value::Record(v) => serde_json::Value::Object(
            v.iter()
                .map(|(k, v)| Ok((k.clone(), json(v)?)))
                .collect::<EvalResult<_>>()?,
        ),
        _ => {
            return Err(EvalError::Message(
                "Cached module data must contain JSON values".into(),
            ));
        }
    })
}
pub fn url_value(url: &Url) -> Value {
    record([
        ("raw".into(), Value::Text(url.to_string())),
        (
            "host".into(),
            Value::Text(url.host_str().unwrap_or_default().into()),
        ),
        ("path".into(), Value::Text(url.path().into())),
        ("scheme".into(), Value::Text(url.scheme().into())),
    ])
}
pub(crate) fn field<'a>(value: &'a Value, key: &str) -> EvalResult<&'a Value> {
    if let Value::Record(fields) = value {
        fields
            .get(key)
            .ok_or_else(|| EvalError::Message(format!("Missing field '{key}'")))
    } else {
        Err(EvalError::Expected("a record"))
    }
}
pub(crate) fn list(value: &Value) -> EvalResult<&[Value]> {
    if let Value::List(items) = value {
        Ok(items)
    } else {
        Err(EvalError::Expected("a list"))
    }
}
