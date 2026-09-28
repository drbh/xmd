//! Values: the kinds a note computes, their display and arithmetic. The
//! scalar literals a note's own syntax can spell are `syntax::Literal`; this
//! adds the host objects (timers, tables, plans, forecasts...) a literal can
//! never be.
use crate::error::{EvalError, EvalResult, Overflow};
use crate::lookups_impl::Forecast;
use crate::resources_impl::Resource;
use crate::timers_impl::Timer;
use chrono::{DateTime, FixedOffset, Local, Months, NaiveDate};
use common::{Code, Currency, ValueType};
use std::{collections::BTreeMap, path::PathBuf};
use syntax::Literal;
pub(crate) type TaskKey = (PathBuf, usize);

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    List(Vec<Value>),
    Record(BTreeMap<String, Value>),
    Function(std::sync::Arc<crate::functional_impl::Function>),
    /// An explicit note import. Members are evaluated only when read.
    Namespace(super::Namespace),
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
    Timer(std::sync::Arc<Timer>),
    Table(std::sync::Arc<crate::tables_impl::TableValue>),
    Plan(std::sync::Arc<crate::plans_impl::PlanValue>),
}
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
                .map(List),
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
    pub fn date(&self) -> EvalResult<NaiveDate> {
        match self {
            Self::Date(d) => Ok(*d),
            Self::DateTime(d) => Ok(d.with_timezone(&Local).date_naive()),
            _ => Err(EvalError::Expected("a date or appointment time")),
        }
    }
    /// The same value with a code spelled out as text, which is how every
    /// operation but a lookup sees one.
    pub(crate) fn plain(self) -> Self {
        match self {
            Self::Code(code) => Self::Text(code.as_str().into()),
            other => other,
        }
    }
    pub(super) fn scalar(&self) -> Option<f64> {
        match self {
            Self::Number(n) | Self::Ratio(n) => Some(*n),
            Self::Count(n) => Some(*n as f64),
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
pub(crate) fn date_value(s: &str) -> Option<Value> {
    syntax::date_value(s).map(Value::from)
}
pub(crate) use syntax::duration;
pub use syntax::relative_date;

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
