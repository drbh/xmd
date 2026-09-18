//! Values: the kinds a note computes, their literals, display and arithmetic.
use crate::{resources::Resource, timers::Timer};
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
    pub fn property(&self, name: &str) -> Result<Value, String> {
        match name {
            "high" => Ok(Value::Number(self.high)),
            "low" => Ok(Value::Number(self.low)),
            "summary" => Ok(Value::Text(self.summary.clone())),
            "rain" => self
                .precipitation
                .map(Value::Ratio)
                .ok_or("This forecast has no precipitation chance".into()),
            _ => Err(format!("Unknown forecast property '{name}'")),
        }
    }
}
/// A 3–5 letter uppercase name is a code literal (USD, EUR, NVDA), never a
/// reference to a note value.
pub fn is_code(name: &str) -> bool {
    (name.len() == 1 || (3..=5).contains(&name.len()))
        && name.bytes().all(|b| b.is_ascii_uppercase())
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
    Resource(Resource),
    Tasks(Vec<TaskKey>),
    Timer(std::sync::Arc<Timer>),
    Table(std::sync::Arc<crate::tables::TableValue>),
    Plan(std::sync::Arc<crate::plans::PlanValue>),
}
impl Value {
    /// Structural access shared by expressions, query records and list projections.
    pub(crate) fn property(&self, key: &str) -> Result<Self, String> {
        use Value::*;
        match (self, key) {
            (Null, _) => Ok(Null),
            (Record(fields), _) => fields
                .get(key)
                .cloned()
                .ok_or_else(|| format!("Unknown field '{key}'")),
            (List(items), _) => items
                .iter()
                .map(|v| v.property(key))
                .collect::<Result<Vec<_>, _>>()
                .map(List),
            (Timer(v), _) => v.property(key),
            (Forecast(v), _) => v.property(key),
            (Plan(v), _) => v.property(key),
            (Money(amount, _), "amount") => Ok(Number(*amount)),
            (Money(_, currency), "currency") => Ok(Text(currency.as_str().into())),
            (Duration(seconds), "seconds") => Ok(Number(*seconds as f64)),
            (Date(date), "value") => Ok(Text(date.to_string())),
            (DateTime(date), "value") => Ok(Text(date.to_rfc3339())),
            (Ratio(value), "value") => Ok(Number(*value)),
            (Money(..), "type") => Ok(Text("money".into())),
            (Duration(_), "type") => Ok(Text("duration".into())),
            (Date(_), "type") => Ok(Text("date".into())),
            (DateTime(_), "type") => Ok(Text("datetime".into())),
            (Ratio(_), "type") => Ok(Text("ratio".into())),
            _ => Err(format!("Unknown field '{key}' on {}", self.type_name())),
        }
    }
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "Null",
            Self::List(_) => "List",
            Self::Record(_) => "Record",
            Self::Function(_) => "Function",
            Self::Namespace(_) => "Namespace",
            Self::Number(_) => "Number",
            Self::Count(_) => "Count",
            Self::Money(..) => "Money",
            Self::Forecast(_) => "Forecast",
            Self::Ratio(_) => "Ratio",
            Self::Duration(_) => "Duration",
            Self::Date(_) => "Date",
            Self::DateTime(_) => "DateTime",
            Self::Bool(_) => "Boolean",
            Self::Text(_) => "Text",
            Self::Resource(_) => "Resource",
            Self::Tasks(_) => "Checklist",
            Self::Timer(t) if t.limit.is_some() => "Countdown",
            Self::Timer(_) => "Stopwatch",
            Self::Table(_) => "Table",
            Self::Plan(_) => "Plan",
        }
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
            Self::Resource(r) => r.target.clone(),
            Self::Tasks(t) => format!("{} tasks", t.len()),
            Self::Timer(t) => t.display(),
            Self::Table(t) => format!("{} rows · {} columns", t.rows.len(), t.columns.len()),
            Self::Plan(p) => p.objective.display(),
        }
    }
    pub fn date(&self) -> Result<NaiveDate, String> {
        match self {
            Self::Date(d) => Ok(*d),
            Self::DateTime(d) => Ok(d.with_timezone(&Local).date_naive()),
            _ => Err("Expected a date or appointment time".into()),
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
pub fn literal(s: &str) -> Result<Value, String> {
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
            return Err("Number must be finite".into());
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
            .map_err(|e| e.to_string());
    }
    Ok(Value::Text(s.into()))
}

pub(crate) fn binary(op: &str, a: Value, b: Value) -> Result<Value, String> {
    use Value::*;
    if matches!(op, "==" | "!=") {
        let equal = a
            .scalar()
            .zip(b.scalar())
            .map(|(a, b)| a == b)
            .unwrap_or(a == b);
        return Ok(Bool(equal == (op == "==")));
    }
    if let (Bool(a), Bool(b)) = (&a, &b)
        && matches!(op, "&&" | "||")
    {
        return match op {
            "&&" => Ok(Bool(*a && *b)),
            "||" => Ok(Bool(*a || *b)),
            _ => Err("Invalid boolean operator".into()),
        };
    }
    if matches!(op, "<" | "<=" | ">" | ">=") {
        if a == Null || b == Null {
            return Ok(Bool(false));
        }
        let cmp = match (&a, &b) {
            (Text(a), Text(b)) => a.partial_cmp(b),
            (Bool(a), Bool(b)) => a.partial_cmp(b),
            (Date(a), Date(b)) => a.partial_cmp(b),
            (DateTime(a), DateTime(b)) => a.partial_cmp(b),
            (Duration(a), Duration(b)) => a.partial_cmp(b),
            (Money(a, ca), Money(b, cb)) => {
                if ca != cb {
                    return Err(format!(
                        "Cannot compare {ca} with {cb}; convert with to(value, {cb})"
                    ));
                }
                a.partial_cmp(b)
            }
            _ => a
                .scalar()
                .zip(b.scalar())
                .and_then(|(a, b)| a.partial_cmp(&b)),
        }
        .ok_or("Cannot compare these value types")?;
        return Ok(Bool(match op {
            "<" => cmp.is_lt(),
            "<=" => cmp.is_le(),
            ">" => cmp.is_gt(),
            _ => cmp.is_ge(),
        }));
    }
    match (op, &a, &b) {
        ("-", Date(a), Date(b)) => return Ok(Duration((*a - *b).num_seconds())),
        ("+" | "-", Date(a), Duration(m)) => {
            if m % 86400 != 0 {
                return Err(
                    "A date requires whole-day durations; use a date/time for hours".into(),
                );
            }
            let delta = chrono::Duration::try_seconds(*m).ok_or("Duration overflow")?;
            return if op == "+" {
                a.checked_add_signed(delta)
            } else {
                a.checked_sub_signed(delta)
            }
            .map(Date)
            .ok_or("Date overflow".into());
        }
        ("+" | "-", DateTime(a), Duration(m)) => {
            let delta = chrono::Duration::try_seconds(*m).ok_or("Duration overflow")?;
            return if op == "+" {
                a.checked_add_signed(delta)
            } else {
                a.checked_sub_signed(delta)
            }
            .map(DateTime)
            .ok_or("Date/time overflow".into());
        }
        ("-", DateTime(a), DateTime(b)) => return Ok(Duration((*a - *b).num_seconds())),
        ("+" | "-", Duration(a), Duration(b)) => {
            return if op == "+" {
                a.checked_add(*b)
            } else {
                a.checked_sub(*b)
            }
            .map(Duration)
            .ok_or("Duration overflow".into());
        }
        ("/", Duration(a), Duration(b)) => {
            return if *b == 0 {
                Err("Division by zero".into())
            } else {
                Ok(Ratio(*a as f64 / *b as f64))
            };
        }
        ("+", Text(a), Text(b)) => {
            if a.len().saturating_add(b.len()) > 1_048_576 {
                return Err("Text exceeds 1 MiB".into());
            }
            return Ok(Text(format!("{a}{b}")));
        }
        _ => {}
    }
    let currency_a = if let Money(_, c) = &a { Some(*c) } else { None };
    let currency_b = if let Money(_, c) = &b { Some(*c) } else { None };
    if let (Some(ca), Some(cb)) = (currency_a, currency_b)
        && ca != cb
    {
        return Err(format!(
            "Cannot combine {ca} and {cb}; convert with to(value, {cb})"
        ));
    }
    let money_a = currency_a.is_some();
    let money_b = currency_b.is_some();
    let counts = matches!((&a, &b), (Count(_), Count(_)));
    if matches!(op, "*" | "/") {
        let scaled = match (&a, &b) {
            (Duration(m), v) => v.scalar().map(|n| {
                if op == "*" {
                    *m as f64 * n
                } else {
                    *m as f64 / n
                }
            }),
            (v, Duration(m)) if op == "*" => v.scalar().map(|n| *m as f64 * n),
            _ => None,
        };
        if let Some(m) = scaled {
            if !m.is_finite() || m.fract() != 0.0 || m.abs() >= i64::MAX as f64 {
                return Err("Duration must fit in whole seconds".into());
            }
            return Ok(Duration(m as i64));
        }
    }
    let x = if let Money(n, _) = a {
        Some(n)
    } else {
        a.scalar()
    }
    .ok_or("Unsupported arithmetic types")?;
    let y = if let Money(n, _) = b {
        Some(n)
    } else {
        b.scalar()
    }
    .ok_or("Unsupported arithmetic types")?;
    let n = match op {
        "+" => x + y,
        "-" => x - y,
        "*" => x * y,
        "/" => {
            if y == 0.0 {
                return Err("Division by zero".into());
            }
            x / y
        }
        _ => return Err(format!("Unknown operator {op}")),
    };
    if !n.is_finite() {
        return Err("Number overflow".into());
    }
    if op == "*" && money_a && money_b {
        return Err("Cannot multiply two money values".into());
    }
    if op == "/" && !money_a && money_b {
        return Err("Cannot divide a scalar by money".into());
    }
    if op == "/" && (money_a && money_b || counts) {
        return Ok(Ratio(n));
    }
    Ok(match currency_a.or(currency_b) {
        Some(currency) => Money(n, currency),
        None => Number(n),
    })
}

/// Repeat from the previous due date, advancing beyond completion; month repeats
/// retain the original day-of-month so Jan 31 -> Feb 28 -> Mar 31.
pub fn next_occurrence(
    rule: &str,
    anchor: NaiveDate,
    completed: NaiveDate,
) -> Result<NaiveDate, String> {
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
                    .ok_or(
                        "@every supports day, week, month, year, or positive whole-day durations",
                    )?,
            };
            days.checked_mul(n as i64)
                .and_then(chrono::Duration::try_days)
                .and_then(|d| anchor.checked_add_signed(d))
        };
        let candidate = candidate.ok_or("Recurrence date overflow")?;
        if candidate > completed {
            return Ok(candidate);
        }
    }
    Err("Recurrence exceeded its search limit".into())
}
