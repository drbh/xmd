//! Values: the kinds a note computes, closures among them, and their display
//! and JSON forms. The scalar literals a note's own syntax can spell are
//! `syntax::Literal`; this adds the host objects (timers, tables, plans,
//! forecasts...) a literal can never be.
use crate::error::{EvalError, EvalResult, Overflow};
use crate::lookups::Forecast;
use chrono::{DateTime, FixedOffset, Months, NaiveDate};
use common::{Code, Currency, Resource, ValueType};
use serde_json::json;
use std::{
    any::Any,
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use syntax::{Expr, Literal};
pub type TaskKey = (PathBuf, usize);

/// Lists and records are shared: cloning one is a reference count, never a
/// copy of its items, so a note's derived data can be cached and handed to a
/// module as-is. Build one with [`Value::list`] or [`Value::record`]; code
/// that changes one in place goes through `Arc::make_mut`, which copies only
/// when the value is still shared.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    List(Arc<Vec<Value>>),
    Record(Arc<BTreeMap<String, Value>>),
    Function(Arc<Function>),
    /// An explicit note import. Members are evaluated only when read.
    Namespace(Namespace),
    Number(f64),
    Count(usize),
    Money(f64, Currency),
    Forecast(Forecast),
    Ratio(f64),
    /// Whole seconds, including for estimates and date arithmetic.
    Duration(i64),
    Date(NaiveDate),
    DateTime(DateTime<FixedOffset>),
    Bool(bool),
    Text(String),
    /// An uppercase code literal: text, with its shape already known.
    Code(Code),
    Resource(Resource),
    Tasks(Vec<TaskKey>),
    /// An object the evaluator builds and a note only reads: a timer, a table
    /// or a plan. Its kind, display and fields are the object's own.
    Host(Arc<dyn HostObject>),
}
// Values cross threads (the language server evaluates off its I/O thread), so
// sharing a list or record must stay thread-safe.
const _: () = {
    const fn shared<T: Send + Sync>() {}
    shared::<Value>()
};
/// A parsed literal is always one of these values: money, dates, a resource
/// and the rest all become the value kind of the same name.
impl From<Literal> for Value {
    fn from(literal: Literal) -> Self {
        match literal {
            Literal::Resource(r) => Value::Resource(r),
            Literal::Date(d) => Value::Date(d),
            Literal::DateTime(d) => Value::DateTime(d),
            Literal::Duration(d) => Value::Duration(d),
            Literal::Bool(b) => Value::Bool(b),
            Literal::Money(n, c) => Value::Money(n, c),
            Literal::Ratio(n) => Value::Ratio(n),
            Literal::Number(n) => Value::Number(n),
            Literal::Text(s) => Value::Text(s),
        }
    }
}
/// An imported note. Its members are the note's definitions, which the engine
/// resolves lazily by name, so it answers no fields of its own.
#[derive(Clone, Debug, PartialEq)]
pub struct Namespace(pub PathBuf);
impl Namespace {
    pub fn path(&self) -> &Path {
        &self.0
    }
}

/// Host objects: the half of [`Value`] the language does not compute.
///
/// A note computes with language values — numbers, money, durations, dates,
/// text, codes, lists and records. It cannot build a forecast, a checklist, a
/// timer, a table, a plan, an imported note or a link target; those are
/// objects some host owns (the lookup store, the document, the timer module,
/// the solver, the filesystem) and the language only reads from. Every kind of
/// reading is one method here, so the generic sites — `Value::kind`,
/// `Value::display`, `Value::property`, `QueryValue::from_value`, completion's
/// property list and the symbol hover — dispatch once instead of carrying an
/// arm per object. What needs the engine or the link registry to answer (a
/// resource's link fields, a checklist's progress) the evaluator adds on top.
pub trait HostObject: HostEq + std::fmt::Debug + Send + Sync {
    /// The kind a note sees. A timer names itself by what it was built with.
    fn kind(&self) -> ValueType;
    /// The human-readable label a hover, inlay or query result shows.
    fn display(&self) -> String;
    /// Fields the object answers on its own. A namespace's members, a
    /// resource's link data and a checklist's counts need the engine, so they
    /// stay in the evaluator and fall through to this error.
    fn property(&self, key: &str) -> EvalResult<Value> {
        Err(EvalError::UnknownField {
            key: key.into(),
            on: Some(self.kind()),
        })
    }
    /// The names completion offers after `value.` that the object knows
    /// without the link registry.
    fn fields(&self) -> Vec<String> {
        vec![]
    }
    /// The object as plain language values, which is the shape a query record
    /// and its JSON hold. `None` keeps the value itself as the scalar: a
    /// namespace has no shape but its own.
    fn query(&self) -> Option<Value> {
        None
    }
    /// Hover detail beyond the name, kind and display line every symbol gets,
    /// when the object can word it without the engine.
    fn hover(&self) -> Option<String> {
        None
    }
}
/// Equality between host objects of any type: equal when they are the same
/// type and equal as that type, which is what `Value`'s own equality needs.
pub trait HostEq: Any {
    fn host_eq(&self, other: &dyn Any) -> bool;
}
impl<T: PartialEq + Any> HostEq for T {
    fn host_eq(&self, other: &dyn Any) -> bool {
        other.downcast_ref::<T>().is_some_and(|other| self == other)
    }
}
impl PartialEq for dyn HostObject {
    fn eq(&self, other: &Self) -> bool {
        self.host_eq(other as &dyn Any)
    }
}
impl dyn HostObject {
    /// The object as the concrete type it was built with.
    pub(crate) fn downcast_ref<T: HostObject>(&self) -> Option<&T> {
        (self as &dyn Any).downcast_ref()
    }
}
impl Value {
    /// A list value holding these items.
    pub fn list(items: Vec<Value>) -> Self {
        Self::List(Arc::new(items))
    }
    /// A record value holding these fields.
    pub fn record(fields: BTreeMap<String, Value>) -> Self {
        Self::Record(Arc::new(fields))
    }
    /// The object behind a host value, or `None` for a value the language
    /// computes. This is the one place every variant is sorted into the two
    /// halves, so it is spelled out rather than using a wildcard: a new kind
    /// has to say which half it belongs to, and `kind` and `display` may then
    /// treat anything left over as impossible.
    pub fn host(&self) -> Option<&dyn HostObject> {
        match self {
            Self::Forecast(forecast) => Some(forecast),
            Self::Tasks(tasks) => Some(tasks),
            Self::Namespace(note) => Some(note),
            Self::Resource(resource) => Some(resource),
            Self::Host(object) => Some(&**object),
            Self::Null
            | Self::List(_)
            | Self::Record(_)
            | Self::Function(_)
            | Self::Number(_)
            | Self::Count(_)
            | Self::Money(..)
            | Self::Ratio(_)
            | Self::Duration(_)
            | Self::Date(_)
            | Self::DateTime(_)
            | Self::Bool(_)
            | Self::Text(_)
            | Self::Code(_) => None,
        }
    }
    /// The object behind a [`Value::Host`], as the concrete type it was built
    /// with: a timer, a table or a plan.
    pub fn downcast<T: HostObject>(&self) -> Option<&T> {
        match self {
            Self::Host(object) => object.downcast_ref(),
            _ => None,
        }
    }
    /// The same, sharing the object rather than borrowing it.
    pub fn downcast_arc<T: HostObject>(&self) -> Option<Arc<T>> {
        match self {
            Self::Host(object) => (object.clone() as Arc<dyn Any + Send + Sync>)
                .downcast()
                .ok(),
            _ => None,
        }
    }
}
pub fn optional(value: Option<Value>) -> Value {
    value.unwrap_or(Value::Null)
}
/// A checklist: the tasks under one heading, which only the counting built-ins
/// and `@after` read, and which the engine resolves against the document.
impl HostObject for Vec<TaskKey> {
    fn kind(&self) -> ValueType {
        ValueType::Checklist
    }
    fn display(&self) -> String {
        format!("{} tasks", self.len())
    }
    fn query(&self) -> Option<Value> {
        Some(Value::list(
            self.iter()
                .map(|(path, index)| {
                    record([
                        ("path", Value::Text(path.to_string_lossy().into())),
                        ("task_index", Value::Count(*index)),
                    ])
                })
                .collect(),
        ))
    }
}
impl HostObject for Namespace {
    fn kind(&self) -> ValueType {
        ValueType::Namespace
    }
    fn display(&self) -> String {
        format!("import(\"{}\")", self.0.display())
    }
}
impl HostObject for Resource {
    fn kind(&self) -> ValueType {
        ValueType::Resource
    }
    fn display(&self) -> String {
        self.target.clone()
    }
    fn query(&self) -> Option<Value> {
        Some(record([("target", Value::Text(self.target.clone()))]))
    }
}
/// A record built from `(key, value)` pairs, keys written as `&str` or `String`.
pub fn record<K: Into<String>>(fields: impl IntoIterator<Item = (K, Value)>) -> Value {
    Value::record(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
}
/// The unit a linear form carries, so money, durations and plain numbers never
/// mix silently.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::Display)]
pub enum Unit {
    /// Not yet known: the form is only bare variables.
    Any,
    Number,
    Money,
    Duration,
}
/// A closure: its parameters and body, the note it was written in, and the
/// names it captured there.
#[derive(Clone, Debug)]
pub struct Function {
    pub expressions: Option<Arc<BTreeMap<String, Expr>>>,
    /// The workspace a module's closure was written against. Only the
    /// evaluator reads it, so to values it is opaque.
    pub environment: Option<Arc<dyn Any + Send + Sync>>,
    pub params: Vec<String>,
    pub body: Expr,
    pub path: PathBuf,
    pub source: Option<(PathBuf, common::Span)>,
    pub captured: BTreeMap<String, Value>,
}

impl PartialEq for Function {
    fn eq(&self, other: &Self) -> bool {
        self.params == other.params
            && self.body == other.body
            && self.path == other.path
            && self.source == other.source
            && self.captured == other.captured
            && match (&self.environment, &other.environment) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
    }
}

impl Value {
    /// Structural access shared by expressions, query records and list projections.
    pub fn property(&self, key: &str) -> EvalResult<Self> {
        use Value::*;
        if let Some(object) = self.host() {
            return object.property(key);
        }
        match (self, key) {
            (Null, _) => Ok(Null),
            (Record(fields), _) => {
                fields
                    .get(key)
                    .cloned()
                    .ok_or_else(|| EvalError::UnknownField {
                        key: key.into(),
                        on: None,
                    })
            }
            (List(items), _) => items
                .iter()
                .map(|v| v.property(key))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::list),
            // Every other kind answers only for the fields its type owns.
            (value, key) if value.kind().fields().contains(&key) => Ok(match (value, key) {
                (Money(amount, _), "amount") => Number(*amount),
                (Money(_, currency), "currency") => Text(currency.as_str().into()),
                (Duration(seconds), "seconds") => Number(*seconds as f64),
                (Date(date), "value") => Text(date.to_string()),
                (DateTime(date), "value") => Text(date.to_rfc3339()),
                (Ratio(value), "value") => Number(*value),
                // The remaining field these kinds list is `type`, the kind's
                // own name in the lowercase spelling notes compare against.
                _ => Text(value.type_name().to_lowercase()),
            }),
            _ => Err(EvalError::UnknownField {
                key: key.into(),
                on: Some(self.kind()),
            }),
        }
    }
    pub fn kind(&self) -> ValueType {
        if let Some(object) = self.host() {
            return object.kind();
        }
        match self {
            Self::Null => ValueType::Null,
            Self::List(_) => ValueType::List,
            Self::Record(_) => ValueType::Record,
            Self::Function(_) => ValueType::Function,
            Self::Number(_) => ValueType::Number,
            Self::Count(_) => ValueType::Count,
            Self::Money(..) => ValueType::Money,
            Self::Ratio(_) => ValueType::Ratio,
            Self::Duration(_) => ValueType::Duration,
            Self::Date(_) => ValueType::Date,
            Self::DateTime(_) => ValueType::DateTime,
            Self::Bool(_) => ValueType::Boolean,
            // A code is a kind of text, and notes compare `type` against it.
            Self::Text(_) | Self::Code(_) => ValueType::Text,
            // Every remaining kind is a host object, answered above.
            _ => unreachable!("Value::host must sort every kind"),
        }
    }
    /// The language-level name of this kind, as notes and queries compare it.
    pub fn type_name(&self) -> &'static str {
        self.kind().as_str()
    }
    /// A round-trippable expression, unlike the human-readable display label.
    /// Only the language's own kinds have one: no note can write down a host
    /// object, so every one of them falls through to `None`.
    pub fn source(&self) -> Option<String> {
        Some(match self {
            Self::Null => "null".into(),
            Self::List(values) => format!(
                "[{}]",
                values
                    .iter()
                    .map(Self::source)
                    .collect::<Option<Vec<_>>>()?
                    .join(", ")
            ),
            Self::Record(fields) => format!(
                "{{{}}}",
                fields
                    .iter()
                    .map(|(k, v)| Some(format!(
                        "{}: {}",
                        serde_json::to_string(k).ok()?,
                        v.source()?
                    )))
                    .collect::<Option<Vec<_>>>()?
                    .join(", ")
            ),
            Self::Number(n) => n.to_string(),
            Self::Money(n, c) => match c.symbol() {
                Some(symbol) if *n < 0.0 => format!("-{symbol}{}", n.abs()),
                Some(symbol) => format!("{symbol}{n}"),
                None => format!("{n} {c}"),
            },
            Self::Ratio(n) if (n * 100.0).is_finite() => format!("{}%", n * 100.0),
            Self::Duration(n) => format!("{n}s"),
            Self::Date(d) => d.to_string(),
            Self::DateTime(d) => d.to_rfc3339(),
            Self::Bool(b) => b.to_string(),
            Self::Text(s) => serde_json::to_string(s).ok()?,
            Self::Code(code) => serde_json::to_string(code.as_str()).ok()?,
            _ => return None,
        })
    }
    pub fn display(&self) -> String {
        if let Some(object) = self.host() {
            return object.display();
        }
        match self {
            Self::Null | Self::List(_) | Self::Record(_) => {
                self.source().unwrap_or_else(|| "<collection>".into())
            }
            Self::Function(_) => "<function>".into(),
            Self::Number(n) => decimal(*n),
            Self::Count(n) => n.to_string(),
            Self::Money(n, c) => money(*n, *c),
            Self::Ratio(n) => format!("{}%", decimal(n * 100.0)),
            Self::Duration(s) => {
                if *s == 0 {
                    "0s".into()
                } else if s % 86400 == 0 {
                    format!("{}d", s / 86400)
                } else if s % 3600 == 0 {
                    format!("{}h", s / 3600)
                } else if s % 60 == 0 {
                    format!("{}m", s / 60)
                } else if s.unsigned_abs() >= 60 {
                    format!(
                        "{}{}m {}s",
                        if *s < 0 { "-" } else { "" },
                        s.unsigned_abs() / 60,
                        s.unsigned_abs() % 60
                    )
                } else {
                    format!("{s}s")
                }
            }
            Self::Date(d) => d.to_string(),
            Self::DateTime(d) => d.format("%Y-%m-%d %H:%M:%S %:z").to_string(),
            Self::Bool(b) => b.to_string(),
            Self::Text(s) => s.clone(),
            Self::Code(code) => code.as_str().into(),
            // Every remaining kind is a host object, answered above.
            _ => unreachable!("Value::host must sort every kind"),
        }
    }
    /// The same value with a code spelled out as text, which is how every
    /// operation but a lookup sees one.
    pub fn plain(self) -> Self {
        match self {
            Self::Code(code) => Self::Text(code.as_str().into()),
            other => other,
        }
    }
    /// A plain number: a number, ratio or count.
    pub fn scalar(&self) -> Option<f64> {
        match self {
            Self::Number(n) | Self::Ratio(n) => Some(*n),
            Self::Count(n) => Some(*n as f64),
            _ => None,
        }
    }
    /// A plain number, or the amount of money.
    pub fn amount(&self) -> Option<f64> {
        match self {
            Self::Money(n, _) => Some(*n),
            other => other.scalar(),
        }
    }
    pub fn currency(&self) -> Option<Currency> {
        match self {
            Self::Money(_, currency) => Some(*currency),
            _ => None,
        }
    }
}
pub(crate) fn decimal(n: f64) -> String {
    let s = format!("{n:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}
fn money(n: f64, currency: Currency) -> String {
    let s = format!("{:.2}", n.abs());
    let (whole, frac) = s.split_once('.').unwrap();
    let mut grouped = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    let cents = if frac == "00" {
        String::new()
    } else {
        format!(".{frac}")
    };
    let sign = if n < 0.0 { "-" } else { "" };
    match currency.symbol() {
        Some(symbol) => format!("{sign}{symbol}{grouped}{cents}"),
        None => format!("{sign}{grouped}{cents} {currency}"),
    }
}
/// A parsed literal, wrapped in the richer value type. Errors are rendered
/// exactly as the syntax layer produced them.
pub fn literal(s: &str) -> EvalResult<Value> {
    syntax::literal(s)
        .map(Value::from)
        .map_err(EvalError::Message)
}
/// A date or timestamp literal, wrapped in the richer value type.
pub fn date_value(s: &str) -> Option<Value> {
    syntax::date_value(s).map(Value::from)
}
pub(crate) use syntax::duration;

/// Repeat from the previous due date, advancing beyond completion; month repeats
/// retain the original day-of-month so Jan 31 -> Feb 28 -> Mar 31.
pub fn next_occurrence(
    rule: &str,
    anchor: NaiveDate,
    completed: NaiveDate,
) -> EvalResult<NaiveDate> {
    let month_step = match rule.trim() {
        "month" | "monthly" => Some(1),
        "year" | "yearly" => Some(12),
        _ => None,
    };
    for n in 1u32..=12000 {
        let candidate = if let Some(step) = month_step {
            anchor.checked_add_months(Months::new(n * step))
        } else {
            let days = match rule.trim() {
                "day" | "daily" => 1,
                "week" | "weekly" => 7,
                s => duration(s)
                    .filter(|d| *d > 0 && *d % 86400 == 0)
                    .map(|d| d / 86400)
                    .ok_or(EvalError::Message(
                        "@every supports day, week, month, year, or positive whole-day durations"
                            .into(),
                    ))?,
            };
            days.checked_mul(n as i64)
                .and_then(chrono::Duration::try_days)
                .and_then(|d| anchor.checked_add_signed(d))
        };
        let candidate = candidate.ok_or(EvalError::Overflowed(Overflow::Recurrence))?;
        if candidate > completed {
            return Ok(candidate);
        }
    }
    Err(EvalError::Message(
        "Recurrence exceeded its search limit".into(),
    ))
}

/// A value as compact JSON: what `debug` shows, and what the catalog's
/// `QueryValue` falls back to for a scalar. A host object describes itself as
/// a plain record first; everything else keeps its own shape.
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
