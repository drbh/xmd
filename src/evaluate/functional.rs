//! Small, pure additions to the shared expression language.
use super::engine::{BinaryOp, Expr, Value};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Clone, Debug)]
pub struct Function {
    pub(crate) expressions: Option<std::sync::Arc<BTreeMap<String, Expr>>>,
    pub(crate) environment: Option<std::sync::Arc<crate::workspace::Workspace>>,
    pub(crate) params: Vec<String>,
    pub(crate) body: Expr,
    pub(crate) path: PathBuf,
    pub(crate) source: Option<(PathBuf, crate::document::Span)>,
    pub(crate) captured: BTreeMap<String, Value>,
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
                (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
                _ => false,
            }
    }
}

pub fn is_builtin(name: &str) -> bool {
    matches!(
        name,
        "parse_date"
            | "parse_datetime"
            | "object"
            | "import"
            | "solve_linear"
            | "entries"
            | "number"
            | "source"
            | "duration_parts"
            | "date_parts"
            | "make_date"
            | "at_time"
            | "parse_time"
            | "parse_duration"
            | "pad_start"
            | "pad_end"
            | "map"
            | "filter"
            | "sort_by"
            | "group_by"
            | "fold"
            | "get"
            | "length"
            | "text"
            | "contains"
            | "starts_with"
            | "ends_with"
            | "split"
            | "join"
            | "lower"
            | "upper"
            | "replace"
            | "slice"
            | "concat"
            | "trim"
            | "type"
            | "floor"
            | "round"
            | "repeat"
            | "format_date"
            | "error"
    )
}

pub fn builtin(name: &str, args: &[Value]) -> Result<Value, String> {
    use Value::*;
    Ok(match (name, args) {
        ("solve_linear", [model]) => crate::evaluate::solver::solve(model)?,
        ("object", [List(entries)]) => {
            let mut fields = BTreeMap::new();
            for entry in entries {
                let Record(entry) = entry else {
                    return Err("object requires key/value records".into());
                };
                let Some(Text(key)) = entry.get("key") else {
                    return Err("object keys must be text".into());
                };
                let value = entry.get("value").ok_or("object entry needs value")?;
                if fields.insert(key.clone(), value.clone()).is_some() {
                    return Err(format!("Duplicate object key '{key}'"));
                }
            }
            Record(fields)
        }
        ("entries", [Record(fields)]) => List(
            fields
                .iter()
                .map(|(key, value)| {
                    Record(
                        [
                            ("key".into(), Text(key.clone())),
                            ("value".into(), value.clone()),
                        ]
                        .into(),
                    )
                })
                .collect(),
        ),
        ("number", [value]) => {
            Number(crate::charts::magnitude(value).ok_or("Expected a numeric value")?)
        }
        ("source", [value]) => Text(
            value
                .source()
                .ok_or("Value cannot be written as an expression")?,
        ),
        ("parse_date", [Text(value), Text(format)]) => {
            chrono::NaiveDate::parse_from_str(value, format)
                .ok()
                .map(Date)
                .unwrap_or(Null)
        }
        ("parse_datetime", [Text(value), Text(format), DateTime(reference)]) => {
            use chrono::TimeZone;
            chrono::NaiveDateTime::parse_from_str(value, format)
                .ok()
                .and_then(|d| reference.offset().from_local_datetime(&d).single())
                .map(DateTime)
                .unwrap_or(Null)
        }
        ("parse_duration", [Text(value)]) => {
            crate::engine::duration(value).map(Duration).unwrap_or(Null)
        }
        ("parse_time", [Text(value), Text(format)]) => {
            use chrono::Timelike;
            chrono::NaiveTime::parse_from_str(value, format)
                .ok()
                .map(|t| Duration(t.num_seconds_from_midnight() as i64))
                .unwrap_or(Null)
        }
        ("make_date", [year, month, day]) => {
            let y = number(year)?;
            let m = number(month)?;
            let d = number(day)?;
            if y.fract() != 0.0
                || m.fract() != 0.0
                || d.fract() != 0.0
                || y < i32::MIN as f64
                || y > i32::MAX as f64
                || !(1.0..=12.0).contains(&m)
                || !(1.0..=31.0).contains(&d)
            {
                Null
            } else {
                chrono::NaiveDate::from_ymd_opt(y as i32, m as u32, d as u32)
                    .map(Date)
                    .unwrap_or(Null)
            }
        }
        ("duration_parts", [Duration(seconds)]) => Record(
            [
                ("hours".into(), Number((seconds / 3600) as f64)),
                ("minutes".into(), Number((seconds / 60 % 60) as f64)),
                ("seconds".into(), Number((seconds % 60) as f64)),
            ]
            .into(),
        ),
        ("date_parts", [value]) => {
            use chrono::Datelike;
            let date = match value {
                Date(d) => *d,
                DateTime(d) => d.date_naive(),
                _ => return Err("date_parts requires a date or timestamp".into()),
            };
            Record(
                [
                    ("year".into(), Number(date.year() as f64)),
                    ("month".into(), Number(date.month() as f64)),
                    ("day".into(), Number(date.day() as f64)),
                    (
                        "weekday".into(),
                        Number(date.weekday().num_days_from_monday() as f64),
                    ),
                ]
                .into(),
            )
        }
        ("at_time", [Date(date), Duration(seconds), DateTime(reference)]) => {
            use chrono::TimeZone;
            if !(0..86400).contains(seconds) {
                return Err("Time must be within a calendar day".into());
            }
            DateTime(
                reference
                    .offset()
                    .from_local_datetime(
                        &date
                            .and_hms_opt(0, 0, 0)
                            .ok_or("Invalid midnight")?
                            .checked_add_signed(chrono::Duration::seconds(*seconds))
                            .ok_or("Date overflow")?,
                    )
                    .single()
                    .ok_or("Invalid timestamp")?,
            )
        }
        ("pad_start" | "pad_end", [Text(value), width, Text(fill)]) => {
            if fill.chars().count() != 1 {
                return Err("Padding must be one character".into());
            }
            let count = index(width)?.saturating_sub(value.chars().count());
            if count > 8192
                || count.saturating_mul(fill.len()).saturating_add(value.len()) > 1_048_576
            {
                return Err("Padding exceeds the size limit".into());
            }
            Text(if name == "pad_start" {
                fill.repeat(count) + value
            } else {
                value.clone() + &fill.repeat(count)
            })
        }
        ("type", [value]) => Text(value.type_name().into()),
        ("error", [Text(message)]) => return Err(message.clone()),
        ("trim", [Text(text)]) => Text(text.trim().into()),
        ("floor" | "round", [value]) => {
            let number = match value {
                Number(n) | Ratio(n) => *n,
                Count(n) => *n as f64,
                _ => return Err(format!("{name} requires a number")),
            };
            Number(if name == "floor" {
                number.floor()
            } else {
                number.round()
            })
        }
        ("concat", lists) => {
            let mut result = Vec::new();
            for list in lists {
                let List(items) = list else {
                    return Err("concat requires lists".into());
                };
                if result.len().saturating_add(items.len()) > 8192 {
                    return Err("List exceeds the collection size limit".into());
                }
                result.extend(items.clone());
            }
            List(result)
        }
        ("slice", [value, start, end]) => {
            let start = index(start)?;
            let end = index(end)?;
            if start > end {
                return Err("slice start must not exceed end".into());
            }
            match value {
                Text(text) => Text(text.chars().skip(start).take(end - start).collect()),
                List(items) => List(items[start.min(items.len())..end.min(items.len())].to_vec()),
                _ => return Err("slice requires text or a list".into()),
            }
        }
        ("repeat", [Text(text), count]) => {
            let count = index(count)?;
            if text.len().saturating_mul(count) > 1_048_576 || count > 8192 {
                return Err("Repeated text exceeds the size limit".into());
            }
            Text(text.repeat(count))
        }
        ("format_date", [value, Text(format)]) => {
            if chrono::format::StrftimeItems::new(format)
                .any(|i| matches!(i, chrono::format::Item::Error))
            {
                return Err("Invalid date format".into());
            }
            Text(match value {
                DateTime(d) => d.format(format).to_string(),
                Date(d) => {
                    // Reject time/offset specifiers for dates rather than panicking in Display.
                    let mut result = String::new();
                    std::fmt::write(&mut result, format_args!("{}", d.format(format)))
                        .map_err(|_| "Format needs a time or timezone")?;
                    result
                }
                _ => return Err("format_date requires a date or timestamp".into()),
            })
        }
        ("get", [Record(fields), Text(key)]) => fields.get(key).cloned().unwrap_or(Null),
        ("get", [List(items), index]) => {
            let index = match index {
                Count(n) => *n,
                Number(n) if n.is_finite() && *n >= 0.0 && n.fract() == 0.0 => *n as usize,
                _ => return Err("List index must be a nonnegative integer".into()),
            };
            items.get(index).cloned().unwrap_or(Null)
        }
        ("get", [Null, _]) => Null,
        ("length", [List(items)]) => Count(items.len()),
        ("length", [Record(fields)]) => Count(fields.len()),
        ("length", [Text(text)]) => Count(text.chars().count()),
        ("text", [Null]) => Null,
        ("text", [value]) => Text(value.display()),
        ("contains", [Text(text), Text(part)]) => Bool(text.contains(part)),
        ("contains", [List(items), value]) => Bool(items.iter().any(|item| {
            super::engine::binary(BinaryOp::Equal, item.clone(), value.clone()) == Ok(Bool(true))
        })),
        ("starts_with", [Text(text), Text(part)]) => Bool(text.starts_with(part)),
        ("ends_with", [Text(text), Text(part)]) => Bool(text.ends_with(part)),
        ("split", [Text(text), Text(separator)]) => {
            let parts = text.split(separator).take(8193).collect::<Vec<_>>();
            if parts.len() > 8192 {
                return Err("List exceeds the collection size limit".into());
            }
            List(parts.into_iter().map(|s| Text(s.into())).collect())
        }
        ("join", [List(items), Text(separator)]) => {
            let parts = items
                .iter()
                .map(|v| match v {
                    Text(s) => Ok(s.as_str()),
                    _ => Err("join requires a list of text"),
                })
                .collect::<Result<Vec<_>, _>>()?;
            let size = parts.iter().map(|s| s.len()).sum::<usize>().saturating_add(
                parts
                    .len()
                    .saturating_sub(1)
                    .saturating_mul(separator.len()),
            );
            if size > 1_048_576 {
                return Err("Text exceeds 1 MiB".into());
            }
            Text(parts.join(separator))
        }
        ("lower", [Text(text)]) => Text(text.to_lowercase()),
        ("upper", [Text(text)]) => Text(text.to_uppercase()),
        ("replace", [Text(text), Text(from), Text(to)]) => {
            let count = text.matches(from).count();
            if count.saturating_mul(to.len()).saturating_add(text.len()) > 1_048_576 {
                return Err("Text exceeds 1 MiB".into());
            }
            Text(text.replace(from, to))
        }
        _ => return Err(format!("Invalid arguments for {name}")),
    })
}

pub(crate) fn check_size(value: &Value) -> Result<(), String> {
    let mut pending = vec![value];
    let mut count = 0usize;
    let mut bytes = 0usize;
    while let Some(value) = pending.pop() {
        count += 1;
        match value {
            Value::List(items) => pending.extend(items),
            Value::Record(fields) => {
                pending.extend(fields.values());
                bytes += fields.keys().map(String::len).sum::<usize>();
            }
            Value::Text(text) => bytes += text.len(),
            _ => (),
        }
        if count + pending.len() > 8192 || bytes > 1_048_576 {
            return Err("Value exceeds the collection or text size limit".into());
        }
    }
    Ok(())
}

fn index(value: &Value) -> Result<usize, String> {
    match value {
        Value::Count(n) => Ok(*n),
        Value::Number(n)
            if n.is_finite() && *n >= 0.0 && n.fract() == 0.0 && *n < usize::MAX as f64 =>
        {
            Ok(*n as usize)
        }
        _ => Err("Expected a nonnegative integer".into()),
    }
}

fn number(value: &Value) -> Result<f64, String> {
    match value {
        Value::Number(n) if n.is_finite() => Ok(*n),
        Value::Count(n) => Ok(*n as f64),
        _ => Err("Expected a finite number".into()),
    }
}

/// Stable scalar ordering, with missing values last (also for descending sorts).
pub(crate) fn compare(a: &Value, b: &Value) -> Result<std::cmp::Ordering, String> {
    use Value::*;
    use std::cmp::Ordering;
    match (a, b) {
        (Null, Null) => Ok(Ordering::Equal),
        (Null, _) => Ok(Ordering::Greater),
        (_, Null) => Ok(Ordering::Less),
        _ => {
            let less = super::engine::binary(BinaryOp::Less, a.clone(), b.clone())?;
            if less == Bool(true) {
                Ok(Ordering::Less)
            } else if super::engine::binary(BinaryOp::Equal, a.clone(), b.clone())? == Bool(true) {
                Ok(Ordering::Equal)
            } else {
                Ok(Ordering::Greater)
            }
        }
    }
}

/// Sum compatible quantities without throwing away their units; missing values are skipped.
pub(crate) fn sum(values: impl IntoIterator<Item = Value>) -> Result<Value, String> {
    use Value::*;
    let mut total = None;
    for value in values {
        if value == Null {
            continue;
        }
        if !matches!(
            value,
            Number(_) | Count(_) | Ratio(_) | Money(..) | Duration(_)
        ) {
            return Err("sum requires numbers, money or durations".into());
        }
        total = Some(match total {
            Some(previous) => super::engine::binary(BinaryOp::Add, previous, value)?,
            None => value,
        });
    }
    Ok(total.unwrap_or(Null))
}
