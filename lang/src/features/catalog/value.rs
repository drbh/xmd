//! `QueryValue`: the JSON-ish value every catalog field holds.
use crate::engine::Value;
use serde::{Serialize, Serializer};
use serde_json::json;
use std::collections::BTreeMap;

/// JSON containers around WTF scalars: units survive filtering, sorting and output.
#[derive(Clone, Debug, PartialEq)]
pub enum QueryValue {
    Null,
    Scalar(Value),
    Array(Vec<QueryValue>),
    Object(BTreeMap<String, QueryValue>),
}
impl QueryValue {
    pub fn text(value: impl Into<String>) -> Self {
        Self::Scalar(Value::Text(value.into()))
    }
    pub fn boolean(value: bool) -> Self {
        Self::Scalar(Value::Bool(value))
    }
    pub fn count(value: usize) -> Self {
        Self::Scalar(Value::Count(value))
    }
    pub fn object<const N: usize>(fields: [(&str, Self); N]) -> Self {
        Self::Object(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }
    pub fn strings(values: impl IntoIterator<Item = String>) -> Self {
        Self::Array(values.into_iter().map(Self::text).collect())
    }
    pub fn from_json(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::Null => Self::Null,
            serde_json::Value::Bool(v) => Self::boolean(v),
            serde_json::Value::Number(v) => v
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .map(Self::count)
                .unwrap_or_else(|| Self::Scalar(Value::Number(v.as_f64().unwrap()))),
            serde_json::Value::String(v) => Self::text(v),
            serde_json::Value::Array(v) => {
                Self::Array(v.into_iter().map(Self::from_json).collect())
            }
            serde_json::Value::Object(v) => Self::Object(
                v.into_iter()
                    .map(|(k, v)| (k, Self::from_json(v)))
                    .collect(),
            ),
        }
    }
    pub fn from_value(value: Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::List(values) => Self::Array(values.into_iter().map(Self::from_value).collect()),
            Value::Record(fields) => Self::Object(
                fields
                    .into_iter()
                    .map(|(k, v)| (k, Self::from_value(v)))
                    .collect(),
            ),
            Value::Plan(p) => Self::object([
                ("goal", Self::text(p.goal.keyword())),
                ("objective", Self::from_value(p.objective.clone())),
                (
                    "variables",
                    Self::Object(
                        p.variables
                            .iter()
                            .map(|(k, v)| (k.clone(), Self::from_value(v.clone())))
                            .collect(),
                    ),
                ),
                (
                    "constraints",
                    Self::Array(
                        p.constraints
                            .iter()
                            .map(|c| {
                                Self::object([
                                    ("name", Self::text(&c.name)),
                                    ("op", Self::text(c.op.as_str())),
                                    ("lhs", Self::from_value(c.lhs.clone())),
                                    ("rhs", Self::from_value(c.rhs.clone())),
                                    ("slack", Self::from_value(c.slack.clone())),
                                    ("binding", Self::boolean(c.binding)),
                                ])
                            })
                            .collect(),
                    ),
                ),
            ]),
            Value::Table(t) => Self::Array(
                t.rows
                    .iter()
                    .map(|row| {
                        Self::Object(
                            t.columns
                                .iter()
                                .cloned()
                                .zip(row.iter().cloned().map(Self::from_value))
                                .collect(),
                        )
                    })
                    .collect(),
            ),
            Value::Timer(t) => Self::object([
                ("state", Self::text(t.state().as_str())),
                ("elapsed", Self::Scalar(Value::Duration(t.elapsed))),
                (
                    "limit",
                    t.limit
                        .map(|n| Self::Scalar(Value::Duration(n)))
                        .unwrap_or(Self::Null),
                ),
                (
                    "started",
                    t.started
                        .map(|d| Self::Scalar(Value::DateTime(d)))
                        .unwrap_or(Self::Null),
                ),
            ]),
            Value::Resource(r) => Self::object([("target", Self::text(r.target))]),
            Value::Forecast(f) => Self::object([
                ("high", Self::Scalar(Value::Number(f.high))),
                ("low", Self::Scalar(Value::Number(f.low))),
                ("summary", Self::text(f.summary)),
                (
                    "rain",
                    f.precipitation
                        .map(|n| Self::Scalar(Value::Ratio(n)))
                        .unwrap_or(Self::Null),
                ),
                ("unit", Self::text(if f.fahrenheit { "F" } else { "C" })),
            ]),
            Value::Tasks(tasks) => Self::Array(
                tasks
                    .into_iter()
                    .map(|(p, i)| {
                        Self::object([
                            ("path", Self::text(p.to_string_lossy())),
                            ("task_index", Self::count(i)),
                        ])
                    })
                    .collect(),
            ),
            scalar => Self::Scalar(scalar),
        }
    }
    pub(crate) fn value(&self) -> Value {
        match self {
            Self::Null => Value::Null,
            Self::Scalar(v) => v.clone(),
            Self::Array(values) => Value::List(values.iter().map(Self::value).collect()),
            Self::Object(fields) => {
                Value::Record(fields.iter().map(|(k, v)| (k.clone(), v.value())).collect())
            }
        }
    }
    pub fn json(&self) -> serde_json::Value {
        match self {
            Self::Null => serde_json::Value::Null,
            Self::Scalar(v) => match v {
                Value::Function(_) => json!({"type":"function"}),
                Value::Namespace(path) => json!({"type":"namespace", "path":path.path()}),
                Value::Number(n) => json!(n),
                Value::Count(n) => json!(n),
                Value::Text(s) => json!(s),
                Value::Bool(b) => json!(b),
                Value::Money(amount, currency) => {
                    json!({"type":"money","amount":amount,"currency":currency.as_str()})
                }
                Value::Duration(seconds) => json!({"type":"duration","seconds":seconds}),
                Value::Date(d) => json!({"type":"date","value":d.to_string()}),
                Value::DateTime(d) => json!({"type":"datetime","value":d.to_rfc3339()}),
                Value::Ratio(n) => json!({"type":"ratio","value":n}),
                other => Self::from_value(other.clone()).json(),
            },
            Self::Array(values) => {
                serde_json::Value::Array(values.iter().map(Self::json).collect())
            }
            Self::Object(values) => serde_json::Value::Object(
                values.iter().map(|(k, v)| (k.clone(), v.json())).collect(),
            ),
        }
    }
    pub fn display(&self) -> String {
        match self {
            Self::Scalar(v) => v.display(),
            Self::Null => "null".into(),
            _ => self.json().to_string(),
        }
    }
    pub fn property(&self, key: &str) -> Result<Self, String> {
        self.value()
            .property(key)
            .map(Self::from_value)
            .map_err(|e| e.to_string())
    }
}
impl Serialize for QueryValue {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.json().serialize(serializer)
    }
}
