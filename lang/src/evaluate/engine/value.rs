//! Values: the kinds a note computes, their literals, display and arithmetic.
use crate::{
    error::{EvalError, EvalResult, Overflow, PropertyOwner},
    resources::Resource,
    timers::Timer,
};
use chrono::{
    DateTime, Datelike, Duration, FixedOffset, Local, Months, NaiveDate, NaiveDateTime, TimeZone,
    Weekday,
};
use std::{collections::BTreeMap, path::PathBuf};
pub type TaskKey = (PathBuf, usize);
/// An ISO 4217 code such as USD or EUR.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Currency([u8; 3]);
impl Currency {
    pub const USD: Currency = Currency(*b"USD");
    pub fn parse(code: &str) -> Option<Self> {
        let bytes = code.as_bytes();
        (bytes.len() == 3 && bytes.iter().all(u8::is_ascii_uppercase))
            .then(|| Currency([bytes[0], bytes[1], bytes[2]]))
    }
    pub fn from_symbol(symbol: char) -> Option<Self> {
        Some(match symbol {
            '$' => Self::USD,
            '€' => Currency(*b"EUR"),
            '£' => Currency(*b"GBP"),
            '¥' => Currency(*b"JPY"),
            _ => return None,
        })
    }
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("???")
    }
    pub fn symbol(&self) -> Option<char> {
        match self.as_str() {
            "USD" => Some('$'),
            "EUR" => Some('€'),
            "GBP" => Some('£'),
            "JPY" => Some('¥'),
            _ => None,
        }
    }
}
impl std::fmt::Display for Currency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
/// A day's weather from a cached lookup.
#[derive(Clone, Debug, PartialEq)]
pub struct Forecast {
    pub high: f64,
    pub low: f64,
    pub summary: String,
    /// Chance of precipitation, 0 to 1, when the source reports it.
    pub precipitation: Option<f64>,
    pub fahrenheit: bool,
}
impl Forecast {
    pub fn display(&self) -> String {
        let unit = if self.fahrenheit { "°F" } else { "°C" };
        let mut s = format!(
            "{}{unit} / {}{unit} · {}",
            decimal(self.high),
            decimal(self.low),
            self.summary
        );
        if let Some(p) = self.precipitation
            && p >= 0.2
        {
            s.push_str(&format!(" · {}% rain", (p * 100.0).round()));
        }
        s
    }
    pub fn property(&self, name: &str) -> EvalResult<Value> {
        match name {
            "high" => Ok(Value::Number(self.high)),
            "low" => Ok(Value::Number(self.low)),
            "summary" => Ok(Value::Text(self.summary.clone())),
            "rain" => self
                .precipitation
                .map(Value::Ratio)
                .ok_or(EvalError::Message(
                    "This forecast has no precipitation chance".into(),
                )),
            _ => Err(EvalError::UnknownProperty {
                owner: PropertyOwner::Forecast,
                name: name.into(),
            }),
        }
    }
}
/// A 3–5 letter uppercase name is a code literal (USD, EUR, NVDA), never a
/// reference to a note value.
pub fn is_code(name: &str) -> bool {
    (name.len() == 1 || (3..=5).contains(&name.len()))
        && name.bytes().all(|b| b.is_ascii_uppercase())
}
/// A code a note writes bare: a currency (USD), a ticker (NVDA), a temperature
/// unit (F). The shape is decided once, where the note is parsed, instead of
/// being read back out of a string at every lookup.
///
/// A code is still *text* to a note: it displays as its letters, `type` calls
/// it Text, and it compares and concatenates with text, because that is what
/// notes and the stdlib already rely on. What it adds is that the evaluator
/// can tell a code from a string that happens to be uppercase.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Code {
    letters: [u8; 5],
    length: u8,
}
impl Code {
    pub fn parse(name: &str) -> Option<Self> {
        is_code(name).then(|| {
            let mut letters = [0; 5];
            letters[..name.len()].copy_from_slice(name.as_bytes());
            Code {
                letters,
                length: name.len() as u8,
            }
        })
    }
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.letters[..self.length as usize]).unwrap_or("???")
    }
    /// The ISO 4217 currency this code names, if it names one.
    pub fn currency(self) -> Option<Currency> {
        Currency::parse(self.as_str())
    }
}
impl std::fmt::Display for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
impl std::fmt::Debug for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Code({})", self.as_str())
    }
}
/// The kind of a value, named exactly as a note or query sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ValueType {
    Null,
    List,
    Record,
    Function,
    Namespace,
    Number,
    Count,
    Money,
    Forecast,
    Ratio,
    Duration,
    Date,
    DateTime,
    Boolean,
    Text,
    Resource,
    Checklist,
    Countdown,
    Stopwatch,
    Table,
    Plan,
    /// No value has this kind: it types a table's yes/no decision column.
    Choice,
}
impl ValueType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Null => "Null",
            Self::List => "List",
            Self::Record => "Record",
            Self::Function => "Function",
            Self::Namespace => "Namespace",
            Self::Number => "Number",
            Self::Count => "Count",
            Self::Money => "Money",
            Self::Forecast => "Forecast",
            Self::Ratio => "Ratio",
            Self::Duration => "Duration",
            Self::Date => "Date",
            Self::DateTime => "DateTime",
            Self::Boolean => "Boolean",
            Self::Text => "Text",
            Self::Resource => "Resource",
            Self::Checklist => "Checklist",
            Self::Countdown => "Countdown",
            Self::Stopwatch => "Stopwatch",
            Self::Table => "Table",
            Self::Plan => "Plan",
            Self::Choice => "Choice",
        }
    }
    /// The properties every value of this kind has, as `Value::property` reads
    /// them and completion offers them. `Timer`, `Forecast`, `Plan` and
    /// `Record` are missing on purpose: their fields depend on the value (a
    /// countdown has `remaining`, a record has whatever it was built with), so
    /// they answer for themselves instead.
    pub fn fields(self) -> &'static [&'static str] {
        match self {
            Self::Money => &["amount", "currency", "type"],
            Self::Duration => &["seconds", "type"],
            Self::Date | Self::DateTime | Self::Ratio => &["value", "type"],
            _ => &[],
        }
    }
}
impl std::fmt::Display for ValueType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    List(Vec<Value>),
    Record(BTreeMap<String, Value>),
    Function(std::sync::Arc<crate::evaluate::functional::Function>),
    /// An explicit note import. Members are evaluated only when read.
    Namespace(PathBuf),
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
    Table(std::sync::Arc<crate::tables::TableValue>),
    Plan(std::sync::Arc<crate::plans::PlanValue>),
}
impl Value {
    /// Structural access shared by expressions, query records and list projections.
    pub(crate) fn property(&self, key: &str) -> EvalResult<Self> {
        use Value::*;
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
            (Timer(v), _) => v.property(key),
            (Forecast(v), _) => v.property(key),
            (Plan(v), _) => v.property(key),
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
        match self {
            Self::Null => ValueType::Null,
            Self::List(_) => ValueType::List,
            Self::Record(_) => ValueType::Record,
            Self::Function(_) => ValueType::Function,
            Self::Namespace(_) => ValueType::Namespace,
            Self::Number(_) => ValueType::Number,
            Self::Count(_) => ValueType::Count,
            Self::Money(..) => ValueType::Money,
            Self::Forecast(_) => ValueType::Forecast,
            Self::Ratio(_) => ValueType::Ratio,
            Self::Duration(_) => ValueType::Duration,
            Self::Date(_) => ValueType::Date,
            Self::DateTime(_) => ValueType::DateTime,
            Self::Bool(_) => ValueType::Boolean,
            // A code is a kind of text, and notes compare `type` against it.
            Self::Text(_) | Self::Code(_) => ValueType::Text,
            Self::Resource(_) => ValueType::Resource,
            Self::Tasks(_) => ValueType::Checklist,
            Self::Timer(t) if t.limit.is_some() => ValueType::Countdown,
            Self::Timer(_) => ValueType::Stopwatch,
            Self::Table(_) => ValueType::Table,
            Self::Plan(_) => ValueType::Plan,
        }
    }
    /// The language-level name of this kind, as notes and queries compare it.
    pub fn type_name(&self) -> &'static str {
        self.kind().as_str()
    }
    /// A round-trippable expression, unlike the human-readable display label.
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
        match self {
            Self::Namespace(path) => format!("import(\"{}\")", path.display()),
            Self::Null | Self::List(_) | Self::Record(_) => {
                self.source().unwrap_or_else(|| "<collection>".into())
            }
            Self::Function(_) => "<function>".into(),
            Self::Number(n) => decimal(*n),
            Self::Count(n) => n.to_string(),
            Self::Money(n, c) => money(*n, *c),
            Self::Forecast(f) => f.display(),
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
            Self::Resource(r) => r.target.clone(),
            Self::Tasks(t) => format!("{} tasks", t.len()),
            Self::Timer(t) => t.display(),
            Self::Table(t) => format!("{} rows · {} columns", t.rows.len(), t.columns.len()),
            Self::Plan(p) => p.objective.display(),
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
pub fn decimal(n: f64) -> String {
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
pub fn duration(s: &str) -> Option<i64> {
    let (n, unit) = s.split_at(s.char_indices().last()?.0);
    let factor = match unit {
        "s" => 1.0,
        "m" => 60.0,
        "h" => 3600.0,
        "d" => 86400.0,
        "w" => 604800.0,
        _ => return None,
    };
    let n = n.parse::<f64>().ok()? * factor;
    (n.is_finite() && n.fract() == 0.0 && n.abs() < i64::MAX as f64).then_some(n as i64)
}
pub fn date_value(s: &str) -> Option<Value> {
    if let Ok(date) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(Value::Date(date));
    }
    if let Ok(date) = DateTime::parse_from_rfc3339(s) {
        return Some(Value::DateTime(date));
    }
    if let Ok(date) = DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M%:z") {
        return Some(Value::DateTime(date));
    }
    for format in ["%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M"] {
        if let Ok(date) = NaiveDateTime::parse_from_str(s, format) {
            return Local
                .from_local_datetime(&date)
                .single()
                .map(|d| Value::DateTime(d.fixed_offset()));
        }
    }
    None
}
/// Whether `s` names a day relative to some other day (`today`, `tomorrow`,
/// `yesterday`, `next friday`), without needing to know which day that is.
pub fn is_relative_date(s: &str) -> bool {
    let s = s.trim().to_lowercase();
    matches!(s.as_str(), "today" | "tomorrow" | "yesterday")
        || s.strip_prefix("next ").is_some_and(|day| {
            matches!(
                day,
                "monday" | "tuesday" | "wednesday" | "thursday" | "friday" | "saturday" | "sunday"
            )
        })
}
pub fn relative_date(s: &str, today: NaiveDate) -> Option<NaiveDate> {
    let s = s.trim().to_lowercase();
    match s.as_str() {
        "today" => return Some(today),
        "tomorrow" => return today.succ_opt(),
        "yesterday" => return today.pred_opt(),
        _ => {}
    }
    let weekday = match s.strip_prefix("next ")? {
        "monday" => Weekday::Mon,
        "tuesday" => Weekday::Tue,
        "wednesday" => Weekday::Wed,
        "thursday" => Weekday::Thu,
        "friday" => Weekday::Fri,
        "saturday" => Weekday::Sat,
        "sunday" => Weekday::Sun,
        _ => return None,
    };
    let mut delta = (weekday.num_days_from_monday() as i64
        - today.weekday().num_days_from_monday() as i64)
        .rem_euclid(7);
    if delta == 0 {
        delta = 7;
    }
    today.checked_add_signed(Duration::days(delta))
}
pub fn literal(s: &str) -> EvalResult<Value> {
    let s = s.trim();
    if let Some(r) = Resource::parse(s) {
        return Ok(Value::Resource(r));
    }
    if let Some(v) = date_value(s) {
        return Ok(v);
    }
    if let Some(v) = duration(s) {
        return Ok(Value::Duration(v));
    }
    if s == "true" || s == "false" {
        return Ok(Value::Bool(s == "true"));
    }
    // Money: a symbol before the number ($3, €450, -£12) or a code after it (700 MXN).
    let (body, code_after) = match s.rsplit_once(' ') {
        Some((body, code)) if Currency::parse(code).is_some() => (body, Currency::parse(code)),
        _ => (s, None),
    };
    let (sign, unsigned) = match body.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", body),
    };
    let symbol = unsigned.chars().next().and_then(Currency::from_symbol);
    let currency = symbol.or(code_after);
    let digits = match symbol {
        Some(_) => &unsigned[unsigned.chars().next().unwrap().len_utf8()..],
        None => unsigned,
    };
    let num = format!("{sign}{}", digits.replace([',', '%'], ""));
    if let Ok(n) = num.parse::<f64>() {
        if !n.is_finite() {
            return Err(EvalError::Message("Number must be finite".into()));
        }
        return Ok(if let Some(currency) = currency {
            Value::Money(n, currency)
        } else if s.ends_with('%') {
            Value::Ratio(n / 100.0)
        } else {
            Value::Number(n)
        });
    }
    if s.starts_with('"') && s.ends_with('"') {
        return serde_json::from_str::<String>(s)
            .map(Value::Text)
            .map_err(|e| EvalError::Message(e.to_string()));
    }
    Ok(Value::Text(s.into()))
}

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
