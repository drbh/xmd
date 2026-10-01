//! Small, pure additions to the shared expression language.
use crate::arithmetic::binary;
use crate::error::{EvalError, EvalResult, Limit, Overflow};
use crate::value::{Value, duration, value_json};
use std::collections::BTreeMap;
use syntax::{BinaryOp, Builtin};

/// The most items one value may hold, and the most bytes of text in it.
pub(crate) const MAX_ITEMS: usize = 8192;
pub(crate) const MAX_BYTES: usize = 1_048_576;

/// Answer a built-in whose arguments have already been evaluated.
pub fn builtin(name: Builtin, args: &[Value]) -> EvalResult<Value> {
    use Builtin as B;
    use Value::*;
    Ok(match (name, args) {
        (B::SolveLinear, [model]) => crate::solver::solve(model)?,
        (B::Object, [List(entries)]) => {
            let mut fields = BTreeMap::new();
            for entry in entries.iter() {
                let Record(entry) = entry else {
                    return Err(EvalError::Message(
                        "object requires key/value records".into(),
                    ));
                };
                let Some(Text(key)) = entry.get("key") else {
                    return Err(EvalError::Message("object keys must be text".into()));
                };
                let value = entry
                    .get("value")
                    .ok_or(EvalError::Message("object entry needs value".into()))?;
                if fields.insert(key.clone(), value.clone()).is_some() {
                    return Err(EvalError::Message(format!("Duplicate object key '{key}'")));
                }
            }
            Value::record(fields)
        }
        (B::Entries, [Record(fields)]) => Value::list(
            fields
                .iter()
                .map(|(key, value)| {
                    Value::record(
                        [
                            ("key".into(), Text(key.clone())),
                            ("value".into(), value.clone()),
                        ]
                        .into(),
                    )
                })
                .collect(),
        ),
        (B::Number, [value]) => {
            Number(magnitude(value).ok_or(EvalError::Expected("a numeric value"))?)
        }
        (B::Source, [value]) => Text(value.source().ok_or(EvalError::Message(
            "Value cannot be written as an expression".into(),
        ))?),
        (B::Debug, [value]) => Text(value_json(value).to_string()),
        (B::Quantize, [List(values), levels, low, high]) => quantize(values, levels, low, high)?,
        (B::ParseDate, [Text(value), Text(format)]) => {
            chrono::NaiveDate::parse_from_str(value, format)
                .ok()
                .map(Date)
                .unwrap_or(Null)
        }
        (B::ParseDatetime, [Text(value), Text(format), DateTime(reference)]) => {
            use chrono::TimeZone;
            chrono::NaiveDateTime::parse_from_str(value, format)
                .ok()
                .and_then(|d| reference.offset().from_local_datetime(&d).single())
                .map(DateTime)
                .unwrap_or(Null)
        }
        (B::NextOccurrence, [Text(rule), Date(anchor), Date(after)]) => {
            let (date, error) = match crate::value::next_occurrence(rule, *anchor, *after) {
                Ok(date) => (Date(date), Null),
                Err(error) => (Null, Text(error.to_string())),
            };
            Value::record([("date".into(), date), ("error".into(), error)].into())
        }
        (B::ToJson, [value]) => Text(
            crate::value::json(value)
                .map_err(|_| {
                    EvalError::Message(
                        "to_json takes text, numbers, Booleans, null, lists and records".into(),
                    )
                })?
                .to_string(),
        ),
        (B::EndPosition, [Text(text)]) => {
            // As an editor counts: lines, and UTF-16 units on the last one.
            let (line, character) = if text.ends_with('\n') {
                (text.lines().count(), 0)
            } else {
                let last = text.lines().last().unwrap_or("");
                (
                    text.lines().count().saturating_sub(1),
                    last.encode_utf16().count(),
                )
            };
            Value::record(
                [
                    ("line".into(), Number(line as f64)),
                    ("character".into(), Number(character as f64)),
                ]
                .into(),
            )
        }
        (B::ParseDuration, [Text(value)]) => duration(value).map(Duration).unwrap_or(Null),
        (B::ParseTime, [Text(value), Text(format)]) => {
            use chrono::Timelike;
            chrono::NaiveTime::parse_from_str(value, format)
                .ok()
                .map(|t| Duration(t.num_seconds_from_midnight() as i64))
                .unwrap_or(Null)
        }
        (B::MakeDate, [year, month, day]) => {
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
        // Money in any currency, or null when the code is not one: what
        // the prelude's conversions and quotes build their answers with.
        (B::MakeMoney, [amount, Text(code)]) => {
            let amount = number(amount)?;
            common::Currency::parse(code)
                .map(|currency| Money(amount, currency))
                .unwrap_or(Null)
        }
        (B::MakeRatio, [fraction]) => Ratio(number(fraction)?),
        (B::Tagged, [Text(kind), fields, display]) => {
            crate::tagged::Tagged::value(kind, fields, display, None)?
        }
        (B::Tagged, [Text(kind), fields, display, hover]) => {
            crate::tagged::Tagged::value(kind, fields, display, Some(hover))?
        }
        (B::Merge3, [Text(base), Text(ours), Text(theirs)]) => {
            // A line-based three-way merge. Conflicting regions come back
            // marked with <<<<<<<, ======= and >>>>>>> and `clean` false.
            let (clean, text) = if ours == theirs {
                (true, ours.clone())
            } else {
                match diffy::merge(base, ours, theirs) {
                    Ok(text) => (true, text),
                    Err(text) => (false, text),
                }
            };
            Value::record([("clean".into(), Bool(clean)), ("text".into(), Text(text))].into())
        }
        (B::UrlEncode, [Text(value)]) => {
            Text(url::form_urlencoded::byte_serialize(value.as_bytes()).collect())
        }
        (B::DurationParts, [Duration(seconds)]) => Value::record(
            [
                ("hours".into(), Number((seconds / 3600) as f64)),
                ("minutes".into(), Number((seconds / 60 % 60) as f64)),
                ("seconds".into(), Number((seconds % 60) as f64)),
            ]
            .into(),
        ),
        (B::DateParts, [value]) => {
            use chrono::Datelike;
            let date = match value {
                Date(d) => *d,
                DateTime(d) => d.date_naive(),
                _ => {
                    return Err(EvalError::Message(
                        "date_parts requires a date or timestamp".into(),
                    ));
                }
            };
            Value::record(
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
        (B::AtTime, [Date(date), Duration(seconds), DateTime(reference)]) => {
            use chrono::TimeZone;
            if !(0..86400).contains(seconds) {
                return Err(EvalError::Message(
                    "Time must be within a calendar day".into(),
                ));
            }
            DateTime(
                reference
                    .offset()
                    .from_local_datetime(
                        &date
                            .and_hms_opt(0, 0, 0)
                            .ok_or(EvalError::Message("Invalid midnight".into()))?
                            .checked_add_signed(chrono::Duration::seconds(*seconds))
                            .ok_or(EvalError::Overflowed(Overflow::Date))?,
                    )
                    .single()
                    .ok_or(EvalError::Message("Invalid timestamp".into()))?,
            )
        }
        (B::PadStart | B::PadEnd, [Text(value), width, Text(fill)]) => {
            if fill.chars().count() != 1 {
                return Err(EvalError::Message("Padding must be one character".into()));
            }
            let count = index(width)?.saturating_sub(value.chars().count());
            if count > MAX_ITEMS
                || count.saturating_mul(fill.len()).saturating_add(value.len()) > MAX_BYTES
            {
                return Err(EvalError::LimitExceeded(Limit::Padding));
            }
            Text(if name == B::PadStart {
                fill.repeat(count) + value
            } else {
                value.clone() + &fill.repeat(count)
            })
        }
        (B::Type, [value]) => Text(value.type_name().into()),
        (B::Error, [Text(message)]) => return Err(EvalError::Custom(message.clone())),
        (B::Pending, [Text(message)]) => return Err(EvalError::Pending(message.clone())),
        (B::Trim, [Text(text)]) => Text(text.trim().into()),
        (B::Floor | B::Round, [value]) => {
            let number = value
                .scalar()
                .ok_or_else(|| EvalError::Message(format!("{name} requires a number")))?;
            Number(if name == B::Floor {
                number.floor()
            } else {
                number.round()
            })
        }
        (B::Concat, lists) => {
            let mut result = Vec::new();
            for list in lists {
                let List(items) = list else {
                    return Err(EvalError::Message("concat requires lists".into()));
                };
                if result.len().saturating_add(items.len()) > MAX_ITEMS {
                    return Err(EvalError::LimitExceeded(Limit::Collection));
                }
                result.extend(items.iter().cloned());
            }
            Value::list(result)
        }
        (B::Slice, [value, start, end]) => {
            let start = index(start)?;
            let end = index(end)?;
            if start > end {
                return Err(EvalError::Message("slice start must not exceed end".into()));
            }
            match value {
                Text(text) => Text(text.chars().skip(start).take(end - start).collect()),
                List(items) => {
                    Value::list(items[start.min(items.len())..end.min(items.len())].to_vec())
                }
                _ => return Err(EvalError::Message("slice requires text or a list".into())),
            }
        }
        (B::Repeat, [Text(text), count]) => {
            let count = index(count)?;
            if text.len().saturating_mul(count) > MAX_BYTES || count > MAX_ITEMS {
                return Err(EvalError::LimitExceeded(Limit::RepeatedText));
            }
            Text(text.repeat(count))
        }
        (B::FormatDate, [value, Text(format)]) => {
            if chrono::format::StrftimeItems::new(format)
                .any(|i| matches!(i, chrono::format::Item::Error))
            {
                return Err(EvalError::Message("Invalid date format".into()));
            }
            Text(match value {
                DateTime(d) => d.format(format).to_string(),
                Date(d) => {
                    // Reject time/offset specifiers for dates rather than panicking in Display.
                    let mut result = String::new();
                    std::fmt::write(&mut result, format_args!("{}", d.format(format))).map_err(
                        |_| EvalError::Message("Format needs a time or timezone".into()),
                    )?;
                    result
                }
                _ => {
                    return Err(EvalError::Message(
                        "format_date requires a date or timestamp".into(),
                    ));
                }
            })
        }
        (B::Get, [Record(fields), Text(key)]) => fields.get(key).cloned().unwrap_or(Null),
        (B::Get, [List(items), index]) => {
            let index = whole(index).ok_or(EvalError::Message(
                "List index must be a nonnegative integer".into(),
            ))?;
            items.get(index).cloned().unwrap_or(Null)
        }
        (B::Get, [Null, _]) => Null,
        (B::Length, [List(items)]) => Count(items.len()),
        (B::Length, [Record(fields)]) => Count(fields.len()),
        (B::Length, [Text(text)]) => Count(text.chars().count()),
        (B::DisplayWidth, [Text(text)]) => {
            Count(unicode_width::UnicodeWidthStr::width(text.as_str()))
        }
        (B::Text, [Null]) => Null,
        (B::Text, [value]) => Text(value.display()),
        (B::Contains, [Text(text), Text(part)]) => Bool(text.contains(part)),
        (B::Contains, [List(items), value]) => Bool(
            items
                .iter()
                .any(|item| binary(BinaryOp::Equal, item.clone(), value.clone()) == Ok(Bool(true))),
        ),
        (B::StartsWith, [Text(text), Text(part)]) => Bool(text.starts_with(part)),
        (B::EndsWith, [Text(text), Text(part)]) => Bool(text.ends_with(part)),
        (B::Split, [Text(text), Text(separator)]) => {
            let parts = text
                .split(separator)
                .take(MAX_ITEMS + 1)
                .collect::<Vec<_>>();
            if parts.len() > MAX_ITEMS {
                return Err(EvalError::LimitExceeded(Limit::Collection));
            }
            Value::list(parts.into_iter().map(|s| Text(s.into())).collect())
        }
        (B::Join, [List(items), Text(separator)]) => {
            let parts = items
                .iter()
                .map(|v| match v {
                    Text(s) => Ok(s.as_str()),
                    _ => Err(EvalError::Message("join requires a list of text".into())),
                })
                .collect::<Result<Vec<_>, _>>()?;
            let size = parts.iter().map(|s| s.len()).sum::<usize>().saturating_add(
                parts
                    .len()
                    .saturating_sub(1)
                    .saturating_mul(separator.len()),
            );
            if size > MAX_BYTES {
                return Err(EvalError::LimitExceeded(Limit::Text));
            }
            Text(parts.join(separator))
        }
        (B::Lower, [Text(text)]) => Text(text.to_lowercase()),
        (B::Upper, [Text(text)]) => Text(text.to_uppercase()),
        (B::Replace, [Text(text), Text(from), Text(to)]) => {
            let count = text.matches(from).count();
            if count.saturating_mul(to.len()).saturating_add(text.len()) > MAX_BYTES {
                return Err(EvalError::LimitExceeded(Limit::Text));
            }
            Text(text.replace(from, to))
        }
        (B::MatchPattern, [Text(text), Text(pattern)]) => match_pattern(text, pattern)?,
        _ => return Err(EvalError::Message(format!("Invalid arguments for {name}"))),
    })
}

/// `match_pattern(text, pattern)`: the first match of a regular expression,
/// as `{text, start, end, groups}` with offsets in Unicode characters (what
/// `slice` and `length` count), or null. Each named group is `{text, start,
/// end}`, or null when it took no part in the match.
fn match_pattern(text: &str, pattern: &str) -> EvalResult<Value> {
    if text.len() > MAX_BYTES {
        return Err(EvalError::LimitExceeded(Limit::Text));
    }
    let compiled = common::Pattern::cached(pattern).map_err(EvalError::Message)?;
    let Some(found) = compiled.first(text) else {
        return Ok(Value::Null);
    };
    let chars = |byte: usize| Value::Count(text[..byte].chars().count());
    let span = |range: std::ops::Range<usize>| {
        [
            ("text".into(), Value::Text(text[range.clone()].into())),
            ("start".into(), chars(range.start)),
            ("end".into(), chars(range.end)),
        ]
    };
    let groups = compiled
        .group_names()
        .map(|name| {
            let group = found
                .groups
                .iter()
                .find(|(n, _)| *n == name)
                .map_or(Value::Null, |(_, range)| {
                    Value::record(span(range.clone()).into())
                });
            (name.to_owned(), group)
        })
        .collect();
    let mut fields: BTreeMap<String, Value> = span(found.range).into();
    fields.insert("groups".into(), Value::record(groups));
    Ok(Value::record(fields))
}

/// How big a value may grow: how many values it holds, nested ones
/// included, and how many bytes of text and field names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub items: usize,
    pub bytes: usize,
}
impl Size {
    /// What any value may hold, wherever it is built.
    pub const LIMIT: Self = Self {
        items: MAX_ITEMS,
        bytes: MAX_BYTES,
    };
    /// How big `value` is, counting no further than `cap`: the first size
    /// past it is as far as the count goes. A list or record measured in
    /// full keeps its size, so a value built from measured parts is measured
    /// in the time it takes to visit its own fields, and a shared value is
    /// never walked twice.
    pub fn of(value: &Value, cap: Self) -> Self {
        Self::measure(value, cap, 0)
    }
    fn measure(value: &Value, cap: Self, depth: usize) -> Self {
        let known = match value {
            Value::List(items) => items.size(),
            Value::Record(fields) => fields.size(),
            Value::Text(text) => {
                return Self {
                    items: 1,
                    bytes: text.len(),
                };
            }
            _ => return Self { items: 1, bytes: 0 },
        };
        if let Some(size) = known {
            return size;
        }
        // Deep values are walked as they are, without recursion.
        if depth > 64 {
            return Self::walked(value, cap);
        }
        let mut size = Self {
            items: 1,
            bytes: match value {
                Value::Record(fields) => fields.keys().map(String::len).sum(),
                _ => 0,
            },
        };
        let mut add = |child: &Value| {
            let child = Self::measure(child, cap, depth + 1);
            size.items = size.items.saturating_add(child.items);
            size.bytes = size.bytes.saturating_add(child.bytes);
            size.within(cap)
        };
        let complete = match value {
            Value::List(items) => items.iter().all(&mut add),
            Value::Record(fields) => fields.values().all(&mut add),
            _ => true,
        };
        if complete && size.within(cap) {
            remember(value, size);
        }
        size
    }
    /// [`Self::of`] without recursion, reading the sizes already known.
    fn walked(value: &Value, cap: Self) -> Self {
        let mut pending = vec![value];
        let mut size = Self { items: 0, bytes: 0 };
        while let Some(value) = pending.pop() {
            let known = match value {
                Value::List(items) => items.size(),
                Value::Record(fields) => fields.size(),
                _ => None,
            };
            match (value, known) {
                (_, Some(known)) => {
                    size.items = size.items.saturating_add(known.items);
                    size.bytes = size.bytes.saturating_add(known.bytes);
                }
                (Value::List(items), None) => {
                    size.items += 1;
                    pending.extend(items.iter());
                }
                (Value::Record(fields), None) => {
                    size.items += 1;
                    pending.extend(fields.values());
                    size.bytes += fields.keys().map(String::len).sum::<usize>();
                }
                (Value::Text(text), None) => {
                    size.items += 1;
                    size.bytes += text.len();
                }
                _ => size.items += 1,
            }
            if size.items.saturating_add(pending.len()) > cap.items || size.bytes > cap.bytes {
                size.items = size.items.saturating_add(pending.len());
                return size;
            }
        }
        if size.within(cap) {
            remember(value, size);
        }
        size
    }
    /// Whether it is within `limit`.
    pub fn within(self, limit: Self) -> bool {
        self.items <= limit.items && self.bytes <= limit.bytes
    }
}

/// Keep the size of a list or record measured in full.
fn remember(value: &Value, size: Size) {
    match value {
        Value::List(items) => items.measured(size),
        Value::Record(fields) => fields.measured(size),
        _ => (),
    }
}

/// Whether `value` is within the size any value may have.
pub fn check_size(value: &Value) -> EvalResult<()> {
    check_size_within(value, Size::LIMIT)
}
/// Whether `value` is within `limit`.
pub fn check_size_within(value: &Value, limit: Size) -> EvalResult<()> {
    if Size::of(value, limit).within(limit) {
        Ok(())
    } else {
        Err(EvalError::LimitExceeded(Limit::Value))
    }
}

/// A nonnegative whole number; a number past `usize` saturates.
fn whole(value: &Value) -> Option<usize> {
    match value {
        Value::Count(n) => Some(*n),
        Value::Number(n) if n.is_finite() && *n >= 0.0 && n.fract() == 0.0 => Some(*n as usize),
        _ => None,
    }
}

/// A count or position, which no number past `usize` can be.
fn index(value: &Value) -> EvalResult<usize> {
    match value {
        Value::Number(n) if *n >= usize::MAX as f64 => None,
        _ => whole(value),
    }
    .ok_or(EvalError::Expected("a nonnegative integer"))
}

fn number(value: &Value) -> EvalResult<f64> {
    match value {
        Value::Number(n) if n.is_finite() => Ok(*n),
        Value::Count(n) => Ok(*n as f64),
        _ => Err(EvalError::Expected("a finite number")),
    }
}

/// Stable scalar ordering, with missing values last (also for descending sorts).
pub fn compare(a: &Value, b: &Value) -> EvalResult<std::cmp::Ordering> {
    use Value::*;
    use std::cmp::Ordering;
    match (a, b) {
        (Null, Null) => Ok(Ordering::Equal),
        (Null, _) => Ok(Ordering::Greater),
        (_, Null) => Ok(Ordering::Less),
        _ => {
            let less = binary(BinaryOp::Less, a.clone(), b.clone())?;
            if less == Bool(true) {
                Ok(Ordering::Less)
            } else if binary(BinaryOp::Equal, a.clone(), b.clone())? == Bool(true) {
                Ok(Ordering::Equal)
            } else {
                Ok(Ordering::Greater)
            }
        }
    }
}

/// Sum compatible quantities without throwing away their units; missing values are skipped.
pub fn sum(values: impl IntoIterator<Item = Value>) -> EvalResult<Value> {
    use Value::*;
    let total =
        values
            .into_iter()
            .filter(|value| *value != Null)
            .try_fold(None, |total, value| {
                if !matches!(
                    value,
                    Number(_) | Count(_) | Ratio(_) | Money(..) | Duration(_)
                ) {
                    return Err(EvalError::Message(
                        "sum requires numbers, money or durations".into(),
                    ));
                }
                Ok(Some(match total {
                    Some(previous) => binary(BinaryOp::Add, previous, value)?,
                    None => value,
                }))
            })?;
    Ok(total.unwrap_or(Null))
}

/// `quantize(values, levels, low, high)`: each value's level, from `0` to
/// `levels - 1`, in equal steps between `low` and `high` (the values' own
/// extremes when both are null), clipped to them. A null stays null, and
/// when `low` and `high` are equal every value is the middle level. Units are
/// the caller's to check: this reads magnitudes, as `number` does.
fn quantize(values: &[Value], levels: &Value, low: &Value, high: &Value) -> EvalResult<Value> {
    let invalid = || EvalError::Message("Invalid arguments for quantize".into());
    let levels = whole(levels).filter(|n| *n > 0).ok_or_else(invalid)?;
    let magnitudes = values
        .iter()
        .map(|value| match value {
            Value::Null => Ok(None),
            value => magnitude(value)
                .filter(|n| n.is_finite())
                .map(Some)
                .ok_or(EvalError::Expected("a numeric value")),
        })
        .collect::<EvalResult<Vec<_>>>()?;
    let (low, high) = match (low, high) {
        (Value::Null, Value::Null) => magnitudes
            .iter()
            .flatten()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), n| {
                (low.min(*n), high.max(*n))
            }),
        (low, high) => (
            magnitude(low).ok_or_else(invalid)?,
            magnitude(high).ok_or_else(invalid)?,
        ),
    };
    let top = (levels - 1) as f64;
    Ok(Value::list(
        magnitudes
            .into_iter()
            .map(|n| match n {
                None => Value::Null,
                Some(_) if low == high => Value::Count(levels / 2),
                Some(n) => {
                    let n = n.clamp(low.min(high), high.max(low));
                    let span = high - low;
                    let fraction = if span.is_finite() {
                        (n - low) / span
                    } else {
                        // Opposite finite extremes may have an infinite difference.
                        (n / 2.0 - low / 2.0) / (high / 2.0 - low / 2.0)
                    };
                    Value::Count(((fraction * top).round() as usize).min(levels - 1))
                }
            })
            .collect(),
    ))
}

/// A numeric magnitude: text, dates and host objects have none.
fn magnitude(value: &Value) -> Option<f64> {
    match value {
        Value::Duration(s) => Some(*s as f64),
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        other => other.amount(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Offsets count characters, as `slice` does, and a named group that
    /// took no part is null.
    #[test]
    fn match_pattern_counts_characters() {
        let text = |s: &str| Value::Text(s.into());
        let found = builtin(
            Builtin::MatchPattern,
            &[text("☎ Ada: 42"), text(r"(?<name>\w+): (?<n>\d+)(?<x>!)?")],
        )
        .unwrap();
        let Value::Record(fields) = &found else {
            panic!("{found:?}")
        };
        assert_eq!(fields["start"], Value::Count(2));
        assert_eq!(fields["end"], Value::Count(9));
        let Value::Record(groups) = &fields["groups"] else {
            panic!("{fields:?}")
        };
        let Value::Record(name) = &groups["name"] else {
            panic!("{groups:?}")
        };
        assert_eq!(name["text"], text("Ada"));
        assert_eq!(name["start"], Value::Count(2));
        assert_eq!(groups["x"], Value::Null);
        let none = [text("abc"), text(r"\d")];
        assert_eq!(builtin(Builtin::MatchPattern, &none).unwrap(), Value::Null);
        let bad = [text("abc"), text("(")];
        assert!(builtin(Builtin::MatchPattern, &bad).is_err());
    }

    /// Levels span the data or the bounds, clipped; gaps stay gaps and a flat
    /// series sits on the middle level.
    #[test]
    fn quantize_scales_clips_and_keeps_gaps() {
        let levels = |values: Vec<Value>, low: Value, high: Value| {
            let Value::List(levels) = builtin(
                Builtin::Quantize,
                &[Value::list(values), Value::Count(8), low, high],
            )
            .unwrap() else {
                panic!("not a list")
            };
            levels
                .iter()
                .map(|v| match v {
                    Value::Count(n) => Some(*n),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let n = Value::Number;
        assert_eq!(
            levels(
                vec![n(12.0), n(18.0), n(9.0), n(24.0)],
                Value::Null,
                Value::Null
            ),
            [Some(1), Some(4), Some(0), Some(7)]
        );
        assert_eq!(
            levels(vec![n(-10.0), Value::Null, n(120.0)], n(0.0), n(100.0)),
            [Some(0), None, Some(7)]
        );
        assert_eq!(
            levels(vec![n(5.0), n(5.0)], Value::Null, Value::Null),
            [Some(4), Some(4)]
        );
        let text = [
            Value::list(vec![Value::Text("x".into())]),
            Value::Count(8),
            Value::Null,
            Value::Null,
        ];
        assert!(builtin(Builtin::Quantize, &text).is_err());
    }
}
