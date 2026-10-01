//! Small, pure additions to the shared expression language.
use crate::arithmetic::binary;
use crate::error::{EvalError, EvalResult, Limit, Overflow};
use crate::value::{Value, duration, record, value_json};
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
                    return Err("object requires key/value records".into());
                };
                let Some(Text(key)) = entry.get("key") else {
                    return Err("object keys must be text".into());
                };
                let value = entry.get("value").ok_or("object entry needs value")?;
                if fields.insert(key.clone(), value.clone()).is_some() {
                    return Err(format!("Duplicate object key '{key}'").into());
                }
            }
            Value::record(fields)
        }
        (B::Entries, [Record(fields)]) => Value::list(
            fields
                .iter()
                .map(|(key, value)| record([("key", Text(key.clone())), ("value", value.clone())]))
                .collect(),
        ),
        (B::Number, [value]) => {
            Number(magnitude(value).ok_or(EvalError::Expected("a numeric value"))?)
        }
        (B::Source, [value]) => Text(
            value
                .source()
                .ok_or("Value cannot be written as an expression")?,
        ),
        (B::Debug, [value]) => Text(value_json(value).to_string()),
        (B::Quantize, [List(values), levels, low, high]) => quantize(values, levels, low, high)?,
        (B::ParseDate, [Text(value), Text(format)]) => {
            chrono::NaiveDate::parse_from_str(value, format)
                .ok()
                .map_or(Null, Date)
        }
        (B::ParseDatetime, [Text(value), Text(format), DateTime(reference)]) => {
            use chrono::TimeZone;
            chrono::NaiveDateTime::parse_from_str(value, format)
                .ok()
                .and_then(|d| reference.offset().from_local_datetime(&d).single())
                .map_or(Null, DateTime)
        }
        (B::NextOccurrence, [Text(rule), Date(anchor), Date(after)]) => {
            let (date, error) = match crate::value::next_occurrence(rule, *anchor, *after) {
                Ok(date) => (Date(date), Null),
                Err(error) => (Null, Text(error.to_string())),
            };
            record([("date", date), ("error", error)])
        }
        (B::ToJson, [value]) => Text(
            crate::value::json(value)
                .map_err(|_| "to_json takes text, numbers, Booleans, null, lists and records")?
                .to_string(),
        ),
        (B::EndPosition, [Text(text)]) => {
            // As an editor counts: lines, and UTF-16 units on the last one.
            let lines = text.lines().count();
            let (line, character) = if text.ends_with('\n') {
                (lines, 0)
            } else {
                let last = text.lines().last().unwrap_or("");
                (lines.saturating_sub(1), last.encode_utf16().count())
            };
            record([
                ("line", Number(line as f64)),
                ("character", Number(character as f64)),
            ])
        }
        (B::ParseDuration, [Text(value)]) => duration(value).map(Duration).unwrap_or(Null),
        (B::ParseTime, [Text(value), Text(format)]) => {
            use chrono::Timelike;
            chrono::NaiveTime::parse_from_str(value, format)
                .ok()
                .map_or(Null, |t| Duration(t.num_seconds_from_midnight() as i64))
        }
        (B::MakeDate, [year, month, day]) => {
            // Each part a whole number in its range, or the date is null.
            let part = |n: &Value, low: f64, high: f64| {
                number(n).map(|n| Some(n).filter(|n| n.fract() == 0.0 && (low..=high).contains(n)))
            };
            let y = part(year, i32::MIN as f64, i32::MAX as f64)?;
            let m = part(month, 1.0, 12.0)?;
            let d = part(day, 1.0, 31.0)?;
            y.zip(m)
                .zip(d)
                .and_then(|((y, m), d)| {
                    chrono::NaiveDate::from_ymd_opt(y as i32, m as u32, d as u32)
                })
                .map_or(Null, Date)
        }
        // Money in any currency, or null when the code is not one: what
        // the prelude's conversions and quotes build their answers with.
        (B::MakeMoney, [amount, Text(code)]) => {
            let amount = number(amount)?;
            common::Currency::parse(code).map_or(Null, |currency| Money(amount, currency))
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
            record([("clean", Bool(clean)), ("text", Text(text))])
        }
        (B::UrlEncode, [Text(value)]) => {
            Text(url::form_urlencoded::byte_serialize(value.as_bytes()).collect())
        }
        (B::DurationParts, [Duration(seconds)]) => record([
            ("hours", Number((seconds / 3600) as f64)),
            ("minutes", Number((seconds / 60 % 60) as f64)),
            ("seconds", Number((seconds % 60) as f64)),
        ]),
        (B::DateParts, [value]) => {
            use chrono::Datelike;
            let date = match value {
                Date(d) => *d,
                DateTime(d) => d.date_naive(),
                _ => return Err("date_parts requires a date or timestamp".into()),
            };
            record([
                ("year", Number(date.year() as f64)),
                ("month", Number(date.month() as f64)),
                ("day", Number(date.day() as f64)),
                (
                    "weekday",
                    Number(date.weekday().num_days_from_monday() as f64),
                ),
            ])
        }
        (B::AtTime, [Date(date), Duration(seconds), DateTime(reference)]) => {
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
                            .ok_or(EvalError::Overflowed(Overflow::Date))?,
                    )
                    .single()
                    .ok_or("Invalid timestamp")?,
            )
        }
        (B::PadStart | B::PadEnd, [Text(value), width, Text(fill)]) => {
            if fill.chars().count() != 1 {
                return Err("Padding must be one character".into());
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
                .ok_or_else(|| format!("{name} requires a number"))?;
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
                    return Err("concat requires lists".into());
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
                return Err("slice start must not exceed end".into());
            }
            match value {
                Text(text) => Text(text.chars().skip(start).take(end - start).collect()),
                List(items) => {
                    Value::list(items[start.min(items.len())..end.min(items.len())].to_vec())
                }
                _ => return Err("slice requires text or a list".into()),
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
        (B::Get, [Record(fields), Text(key)]) => fields.get(key).cloned().unwrap_or(Null),
        (B::Get, [List(items), index]) => {
            let index = whole(index).ok_or("List index must be a nonnegative integer")?;
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
                    _ => Err(EvalError::from("join requires a list of text")),
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
        _ => return Err(format!("Invalid arguments for {name}").into()),
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
    pub const LIMIT: Self = Self::new(MAX_ITEMS, MAX_BYTES);
    /// `items` values and `bytes` bytes.
    pub const fn new(items: usize, bytes: usize) -> Self {
        Self { items, bytes }
    }
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
            Value::Text(text) => return Self::new(1, text.len()),
            _ => return Self { items: 1, bytes: 0 },
        };
        if let Some(size) = known {
            return size;
        }
        // Deep values are walked as they are, without recursion.
        if depth > 64 {
            return Self::walked(value, cap);
        }
        let mut size = Self::new(1, 0);
        if let Value::Record(fields) = value {
            size.bytes = fields.keys().map(String::len).sum();
        }
        let mut add = |child: &Value| {
            size = size + Self::measure(child, cap, depth + 1);
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
                (_, Some(known)) => size = size + known,
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
    /// Whether `value` is within this limit.
    pub fn check(self, value: &Value) -> EvalResult<()> {
        let within = Self::of(value, self).within(self);
        within
            .then_some(())
            .ok_or(EvalError::LimitExceeded(Limit::Value))
    }
}
/// Sizes add up, saturating.
impl std::ops::Add for Size {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            items: self.items.saturating_add(other.items),
            bytes: self.bytes.saturating_add(other.bytes),
        }
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
    Size::LIMIT.check(value)
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
                    return Err(EvalError::from("sum requires numbers, money or durations"));
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
    let invalid = || EvalError::from("Invalid arguments for quantize");
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
