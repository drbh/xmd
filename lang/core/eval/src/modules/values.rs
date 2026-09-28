//! Converting between module values and the JSON and URLs hosts exchange.
use crate::engine_impl::Value;
use crate::error::{EvalError, EvalResult};
use crate::records::{ToValue, UrlRecord};
use url::Url;

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
pub(crate) fn url_value(url: &Url) -> Value {
    UrlRecord::from(url).to_value()
}
pub(crate) fn list(value: &Value) -> EvalResult<&[Value]> {
    if let Value::List(items) = value {
        Ok(items)
    } else {
        Err(EvalError::Expected("a list"))
    }
}
