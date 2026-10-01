//! Scalar literals: the shapes a note's own syntax can spell out,
//! independent of how the evaluator represents a value.
use chrono::{DateTime, Datelike, FixedOffset, Local, NaiveDate, NaiveDateTime, TimeZone};
use common::{Currency, Resource};

/// A scalar literal, exactly as a note's own syntax can spell one: the shapes
/// [`literal`] recognizes, before the evaluator wraps one in its richer value
/// type (which also holds host objects a literal can never be).
#[derive(Clone, Debug, PartialEq)]
pub enum Literal {
    Resource(Resource),
    Date(NaiveDate),
    DateTime(DateTime<FixedOffset>),
    Duration(i64),
    Bool(bool),
    Money(f64, Currency),
    Ratio(f64),
    Number(f64),
    Text(String),
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
pub fn date_value(s: &str) -> Option<Literal> {
    if let Ok(date) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(Literal::Date(date));
    }
    if let Ok(date) = DateTime::parse_from_rfc3339(s) {
        return Some(Literal::DateTime(date));
    }
    if let Ok(date) = DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M%:z") {
        return Some(Literal::DateTime(date));
    }
    for format in ["%Y-%m-%dT%H:%M", "%Y-%m-%d %H:%M"] {
        if let Ok(date) = NaiveDateTime::parse_from_str(s, format) {
            return Local
                .from_local_datetime(&date)
                .single()
                .map(|d| Literal::DateTime(d.fixed_offset()));
        }
    }
    None
}
/// Whether `s` names a day relative to some other day (`today`, `tomorrow`,
/// `yesterday`, `next friday`), without needing to know which day that is.
pub fn is_relative_date(s: &str) -> bool {
    relative_date(s, NaiveDate::default()).is_some()
}
pub fn relative_date(s: &str, today: NaiveDate) -> Option<NaiveDate> {
    const DAYS: [&str; 7] = [
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
        "sunday",
    ];
    let s = s.trim().to_lowercase();
    match s.as_str() {
        "today" => return Some(today),
        "tomorrow" => return today.succ_opt(),
        "yesterday" => return today.pred_opt(),
        _ => {}
    }
    let day = s.strip_prefix("next ")?;
    let weekday = DAYS.iter().position(|d| *d == day)? as i64;
    let mut delta = (weekday - today.weekday().num_days_from_monday() as i64).rem_euclid(7);
    if delta == 0 {
        delta = 7;
    }
    today.checked_add_signed(chrono::Duration::days(delta))
}
pub fn literal(s: &str) -> Result<Literal, String> {
    let s = s.trim();
    if let Some(r) = Resource::parse(s) {
        return Ok(Literal::Resource(r));
    }
    if let Some(v) = date_value(s) {
        return Ok(v);
    }
    if let Some(v) = duration(s) {
        return Ok(Literal::Duration(v));
    }
    if s == "true" || s == "false" {
        return Ok(Literal::Bool(s == "true"));
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
            Literal::Money(n, currency)
        } else if s.ends_with('%') {
            Literal::Ratio(n / 100.0)
        } else {
            Literal::Number(n)
        });
    }
    if s.starts_with('"') && s.ends_with('"') {
        return serde_json::from_str::<String>(s)
            .map(Literal::Text)
            .map_err(|e| e.to_string());
    }
    Ok(Literal::Text(s.into()))
}
