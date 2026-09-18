//! Trip itineraries: day headings, timed stops, and their details, parsed from
//! prose that reads naturally on its own. Shared by the native server,
//! browser adapter and CLI.
use crate::document::{Document, Span};
use crate::{
    engine::Value,
    modules::{from_json, record, standard},
};
use chrono::{NaiveDate, NaiveTime, Timelike, Weekday};
use lsp_types::{Position, Range, TextEdit};

#[derive(Clone, Debug, PartialEq)]
pub struct Detail {
    pub line: usize,
    pub key: String,
    pub key_span: Span,
    pub value: String,
    pub value_span: Span,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Stop {
    pub line: usize,
    /// The stop's kind: written as a marker, or inferred from an emoji or the
    /// first words so formatting can write the marker.
    pub kind: Option<&'static Kind>,
    /// Where the marker sits, when one was written.
    pub marker_span: Option<Span>,
    /// True when the title was recognized from words or emoji, not a marker.
    pub inferred: bool,
    /// One past the last line of the stop's block.
    pub end_line: usize,
    pub time: NaiveTime,
    pub time_span: Span,
    /// The time was written with AM/PM rather than a 24-hour clock.
    pub twelve_hour: bool,
    pub title: String,
    pub title_span: Span,
    pub details: Vec<Detail>,
    /// Free lines under the stop that are not `Key: value` details.
    pub notes: Vec<Span>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Day {
    pub line: usize,
    pub end_line: usize,
    pub weekday: Option<(Weekday, Span)>,
    pub month: u32,
    pub day: u32,
    pub year: Option<i32>,
    pub date_span: Span,
    pub places: Option<(String, Span)>,
    pub stops: Vec<Stop>,
}

const WEEKDAYS: [(&str, Weekday); 7] = [
    ("monday", Weekday::Mon),
    ("tuesday", Weekday::Tue),
    ("wednesday", Weekday::Wed),
    ("thursday", Weekday::Thu),
    ("friday", Weekday::Fri),
    ("saturday", Weekday::Sat),
    ("sunday", Weekday::Sun),
];
const MONTHS: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];
/// A kind of stop, written as a one-character ASCII marker after the time.
#[derive(Debug, PartialEq, Eq)]
pub struct Kind {
    pub marker: char,
    pub name: &'static str,
    /// Leading words that imply the kind when no marker is written.
    pub words: &'static [&'static str],
    /// Emoji that Format Document converts into the marker.
    pub emoji: &'static [&'static str],
}
pub const KINDS: &[Kind] = &[
    Kind {
        marker: '>',
        name: "Depart",
        words: &["depart", "departure", "fly", "flight", "leave", "board"],
        emoji: &["🛫", "✈️", "✈", "🚆", "🚄", "🚢"],
    },
    Kind {
        marker: '<',
        name: "Arrive",
        words: &["arrive", "arrival", "land"],
        emoji: &["🛬"],
    },
    Kind {
        marker: '~',
        name: "Transit",
        words: &[
            "transit", "taxi", "walk", "bus", "drive", "transfer", "uber", "metro", "ferry",
        ],
        emoji: &["🚕", "🚶", "🚌", "🚗", "🚇", "🛵", "🚲"],
    },
    Kind {
        marker: '@',
        name: "Stay",
        words: &[
            "check in",
            "check-in",
            "checkin",
            "check out",
            "check-out",
            "checkout",
            "hotel",
            "stay",
        ],
        emoji: &["🛏️", "🛏", "🏨", "🧳"],
    },
    Kind {
        marker: '*',
        name: "Meal",
        words: &[
            "breakfast",
            "brunch",
            "lunch",
            "dinner",
            "coffee",
            "drinks",
            "food",
            "eat",
            "grab food",
            "snack",
            "tasting",
        ],
        emoji: &["🍽️", "🍽", "☕", "🍷", "🍺", "🍸", "🥐", "🌮"],
    },
    Kind {
        marker: '+',
        name: "Visit",
        words: &[
            "visit", "tour", "see", "museum", "show", "concert", "hike", "class",
        ],
        emoji: &["🌳", "🎟️", "🎟", "🏛️", "🏛", "🎭", "🎶", "🥾"],
    },
    Kind {
        marker: '?',
        name: "Explore",
        words: &["explore", "wander", "free time", "browse", "shop"],
        emoji: &["🔍", "🗺️", "🗺", "🛍️"],
    },
];
pub fn kind_for(marker: char) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.marker == marker)
}
const DECORATIONS: &[&str] = &["🌟", "⭐", "❗", "✨"];
/// Strip a leading emoji or skin-tone modifier sequence, returning the kind
/// it implied and the rest of the title.
fn strip_emoji(title: &str) -> (Option<&'static Kind>, &str) {
    let mut kind = None;
    let mut rest = title;
    loop {
        let before = rest;
        for k in KINDS {
            for e in k.emoji {
                if let Some(tail) = rest.strip_prefix(e) {
                    kind = kind.or(Some(k));
                    rest = tail;
                }
            }
        }
        for d in DECORATIONS {
            if let Some(tail) = rest.strip_prefix(d) {
                rest = tail;
            }
        }
        // Skin tones and variation selectors ride along with the emoji.
        rest = rest.trim_start_matches(|c: char| {
            matches!(c as u32, 0x1F3FB..=0x1F3FF | 0xFE0F | 0x200D) || c.is_whitespace()
        });
        if rest == before {
            return (kind, rest);
        }
    }
}
/// The kind implied by a title's first words.
fn kind_from_words(title: &str) -> Option<&'static Kind> {
    let lower = title.to_lowercase();
    KINDS.iter().find(|k| {
        k.words.iter().any(|w| {
            lower.starts_with(w)
                && lower[w.len()..]
                    .chars()
                    .next()
                    .is_none_or(|c| !c.is_alphanumeric())
        })
    })
}
/// Detail keys offered by completion inside a stop.
pub const KEYS: &[&str] = &[
    "Address",
    "Reservation Number",
    "Confirmation Number",
    "Seats",
    "Phone",
    "Cancel by",
    "Cost",
    "Note",
];

fn word_prefix(s: &str) -> &str {
    let end = s
        .char_indices()
        .find(|(_, c)| !c.is_alphabetic())
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    &s[..end]
}
/// `Friday, November 20[, 2026][ · places]`, also after `#` marks.
fn day_heading(line: &str, row: usize) -> Option<Day> {
    let indent = line.len() - line.trim_start().len();
    let mut i = indent;
    let hashes = line[i..].bytes().take_while(|b| *b == b'#').count();
    if hashes > 0 {
        i += hashes;
        i += line[i..].len() - line[i..].trim_start().len();
    }
    let lower = line.to_lowercase();
    let mut weekday = None;
    let word = word_prefix(&lower[i..]);
    if let Some((_, wd)) = WEEKDAYS.iter().find(|(name, _)| *name == word) {
        weekday = Some((*wd, Span::new(row, i, i + word.len())));
        i += word.len();
        let rest = &line[i..];
        i += rest.len() - rest.trim_start_matches([',', ' ', '\t']).len();
    }
    let date_start = i;
    let month_word = word_prefix(&lower[i..]);
    let month = MONTHS.iter().position(|m| *m == month_word)? as u32 + 1;
    i += month_word.len();
    let rest = &line[i..];
    let gap = rest.len() - rest.trim_start().len();
    if gap == 0 {
        return None;
    }
    i += gap;
    let digits = line[i..].bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || digits > 2 {
        return None;
    }
    let day: u32 = line[i..i + digits].parse().ok()?;
    i += digits;
    let mut year = None;
    let after = &line[i..];
    let trimmed = after.trim_start_matches([',', ' ', '\t']);
    let year_digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
    if year_digits == 4
        && trimmed[year_digits..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric())
    {
        year = trimmed[..4].parse().ok();
        i += after.len() - trimmed.len() + 4;
    }
    // Only whole words count: "Friday, November 20th" is prose, not a heading.
    if line[i..]
        .chars()
        .next()
        .is_some_and(|c| c.is_alphanumeric())
    {
        return None;
    }
    let date_span = Span::new(row, date_start, i);
    let tail = &line[i..];
    let stripped = tail.trim_start_matches(|c: char| {
        c.is_whitespace() || matches!(c, '·' | '-' | '—' | '–' | '|' | ':')
    });
    let places_start = i + tail.len() - stripped.len();
    let mut places_text = stripped.trim_end();
    // A trailing :name on a heading belongs to the section, not the places.
    if let Some(at) = places_text.rfind(" :")
        && crate::document::identifier(&places_text[at + 2..])
    {
        places_text = places_text[..at].trim_end();
    }
    let places = (!places_text.is_empty()).then(|| {
        (
            places_text.to_string(),
            Span::new(row, places_start, places_start + places_text.len()),
        )
    });
    Some(Day {
        line: row,
        end_line: row + 1,
        weekday,
        month,
        day,
        year,
        date_span,
        places,
        stops: vec![],
    })
}
/// `07:04 AM`, `2:45 PM`, `7 PM`, or `14:30` at the start of a line.
pub fn clock(line: &str, row: usize) -> Option<(NaiveTime, Span, bool, usize)> {
    let indent = line.len() - line.trim_start().len();
    let s = &line[indent..];
    let hour_digits = s.bytes().take_while(u8::is_ascii_digit).count();
    if hour_digits == 0 || hour_digits > 2 {
        return None;
    }
    let hour: u32 = s[..hour_digits].parse().ok()?;
    let mut i = hour_digits;
    let mut minute = 0;
    let mut had_colon = false;
    if s[i..].starts_with(':') {
        had_colon = true;
        let digits = s[i + 1..].bytes().take_while(u8::is_ascii_digit).count();
        if digits != 2 {
            return None;
        }
        minute = s[i + 1..i + 3].parse().ok()?;
        i += 3;
    }
    let after = &s[i..];
    let space = after.len() - after.trim_start().len();
    let meridiem = after[space..].get(..2).map(str::to_ascii_uppercase);
    let twelve = matches!(meridiem.as_deref(), Some("AM" | "PM"))
        && after[space + 2..]
            .chars()
            .next()
            .is_none_or(|c| c.is_whitespace());
    if !twelve && !had_colon {
        return None;
    }
    let mut hour24 = hour;
    if twelve {
        if !(1..=12).contains(&hour) {
            return None;
        }
        hour24 = hour % 12
            + if meridiem.as_deref() == Some("PM") {
                12
            } else {
                0
            };
        i += space + 2;
    } else if hour > 23 {
        return None;
    }
    if minute > 59 {
        return None;
    }
    let rest = &s[i..];
    let gap = rest.len() - rest.trim_start().len();
    if gap == 0 && !rest.is_empty() {
        return None;
    }
    Some((
        NaiveTime::from_hms_opt(hour24, minute, 0)?,
        Span::new(row, indent, indent + i),
        twelve,
        indent + i + gap,
    ))
}
fn detail(line: &str, row: usize) -> Option<Detail> {
    let indent = line.len() - line.trim_start().len();
    let s = &line[indent..];
    let colon = s.find(':')?;
    let key = s[..colon].trim_end();
    if key.is_empty()
        || key.len() > 32
        || !key.chars().next().is_some_and(char::is_alphabetic)
        || !key.chars().all(|c| c.is_alphabetic() || c == ' ')
        || s[colon + 1..].starts_with("//")
    {
        return None;
    }
    let value_raw = &s[colon + 1..];
    let value_start = indent + colon + 1 + value_raw.len() - value_raw.trim_start().len();
    let value = value_raw.trim();
    Some(Detail {
        line: row,
        key: key.to_string(),
        key_span: Span::new(row, indent, indent + key.len()),
        value: value.to_string(),
        value_span: Span::new(row, value_start, value_start + value.len()),
    })
}

pub fn parse(lines: &[&str]) -> Vec<Day> {
    let mut days: Vec<Day> = Vec::new();
    let mut fence = false;
    for (row, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = !fence;
            continue;
        }
        if fence || trimmed.starts_with("<!--") {
            continue;
        }
        if let Some(day) = day_heading(line, row) {
            if let Some(previous) = days.last_mut() {
                previous.end_line = row;
                if let Some(stop) = previous.stops.last_mut() {
                    stop.end_line = stop.end_line.min(row);
                }
            }
            days.push(day);
            continue;
        }
        let Some(day) = days.last_mut() else {
            continue;
        };
        // A heading that is not a day ends the itinerary section.
        if trimmed.starts_with('#') {
            day.end_line = row;
            if let Some(stop) = day.stops.last_mut() {
                stop.end_line = stop.end_line.min(row);
            }
            // Mark the day closed by pushing a sentinel: later stops need a new day.
            days.push(Day {
                line: usize::MAX,
                end_line: usize::MAX,
                weekday: None,
                month: 0,
                day: 0,
                year: None,
                date_span: Span::new(row, 0, 0),
                places: None,
                stops: vec![],
            });
            continue;
        }
        if day.line == usize::MAX {
            continue;
        }
        day.end_line = row + 1;
        if trimmed.starts_with("- [") || trimmed.starts_with('|') || trimmed.starts_with('[') {
            if let Some(stop) = day.stops.last_mut() {
                stop.end_line = stop.end_line.min(row);
            }
            continue;
        }
        if let Some((time, time_span, twelve_hour, title_start)) = clock(line, row) {
            let raw = line[title_start..].trim_end();
            let mut marker_span = None;
            let mut kind = None;
            let mut inferred = false;
            let mut title_offset = 0;
            if let Some(c) = raw.chars().next()
                && let Some(k) = kind_for(c)
                && raw[c.len_utf8()..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace)
            {
                kind = Some(k);
                marker_span = Some(Span::new(row, title_start, title_start + c.len_utf8()));
                let after = &raw[c.len_utf8()..];
                title_offset = c.len_utf8() + after.len() - after.trim_start().len();
            }
            let (from_emoji, stripped) = strip_emoji(&raw[title_offset..]);
            if kind.is_none() {
                kind = from_emoji.or_else(|| kind_from_words(stripped));
                inferred = kind.is_some();
            }
            let title_start = title_start + raw.len() - stripped.len();
            let title = stripped.trim_end();
            day.stops.push(Stop {
                line: row,
                kind,
                marker_span,
                inferred,
                end_line: row + 1,
                time,
                time_span,
                twelve_hour,
                title: title.to_string(),
                title_span: Span::new(row, title_start, title_start + title.len()),
                details: vec![],
                notes: vec![],
            });
            continue;
        }
        let Some(stop) = day.stops.last_mut() else {
            continue;
        };
        if trimmed.is_empty() {
            continue;
        }
        if stop.end_line != row {
            // A blank line closed the stop's block; this is loose prose.
            continue;
        }
        stop.end_line = row + 1;
        if let Some(detail) = detail(line, row) {
            stop.details.push(detail);
        } else {
            let indent = line.len() - trimmed.len();
            stop.notes
                .push(Span::new(row, indent, line.trim_end().len()));
        }
    }
    days.retain(|d| d.line != usize::MAX);
    days
}

/// Calendar dates for each day. Years carry forward from the previous day or
/// an explicit year, and a first day without one is the next occurrence.
pub fn dates(days: &[Day], today: NaiveDate) -> Vec<Option<NaiveDate>> {
    try_dates(days, today).unwrap_or_else(|_| vec![None; days.len()])
}
pub(crate) fn try_dates(days: &[Day], today: NaiveDate) -> Result<Vec<Option<NaiveDate>>, String> {
    let input = Value::List(days.iter().map(day_parts).collect());
    let result = call("dates", vec![input, Value::Date(today)])?;
    Ok(crate::modules::list(&result)?
        .iter()
        .map(|v| match v {
            Value::Date(d) => Some(*d),
            _ => None,
        })
        .collect())
}
pub fn display_time(stop: &Stop) -> String {
    call("time_text", vec![stop_record(stop, None)])
        .map(|v| v.display())
        .unwrap_or_else(|e| e)
}
/// Human-readable travel duration, shared with user modules.
pub fn human(seconds: i64) -> String {
    standard("format", "human", vec![Value::Duration(seconds)], epoch())
        .expect("valid duration")
        .display()
}
pub fn month_name(month: u32) -> &'static str {
    let name = MONTHS[(month as usize).saturating_sub(1).min(11)];
    match name {
        "january" => "January",
        "february" => "February",
        "march" => "March",
        "april" => "April",
        "may" => "May",
        "june" => "June",
        "july" => "July",
        "august" => "August",
        "september" => "September",
        "october" => "October",
        "november" => "November",
        _ => "December",
    }
}
/// Seconds from one stop to the next on the same day, when in order.
pub fn gap(stop: &Stop, next: &Stop) -> Option<i64> {
    match call(
        "gap",
        vec![stop_record(stop, None), stop_record(next, None)],
    ) {
        Ok(Value::Duration(n)) => Some(n),
        _ => None,
    }
}
/// A Google Maps search for an address; every platform opens it.
pub fn map_url(address: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(address.as_bytes()).collect();
    format!("https://www.google.com/maps/search/?api=1&query={encoded}")
}
/// `Cancel by: 24h before` or an explicit datetime, resolved against the stop.
pub fn cancel_by(day: NaiveDate, stop: &Stop) -> Option<(chrono::NaiveDateTime, bool)> {
    let value = call(
        "cancel",
        vec![
            Value::Date(day),
            stop_record(stop, None),
            Value::DateTime(epoch()),
        ],
    )
    .ok()?;
    let at = crate::modules::field(&value, "at").ok()?;
    let Value::DateTime(at) = at else { return None };
    let relative = crate::modules::field(&value, "relative").ok()? == &Value::Bool(true);
    Some((at.naive_local(), relative))
}
pub fn canonical_line(stop: &Stop) -> String {
    call("canonical", vec![stop_record(stop, None)])
        .map(|v| v.display())
        .unwrap_or_else(|e| e)
}
pub fn label(stop: &Stop) -> String {
    call("label", vec![stop_record(stop, None)])
        .map(|v| v.display())
        .unwrap_or_else(|e| e)
}
pub fn formatting(doc: &Document, days: &[Day]) -> Vec<TextEdit> {
    call(
        "format_days",
        vec![Value::List(
            days.iter().map(|d| day_record(d, doc)).collect(),
        )],
    )
    .and_then(|v| crate::modules::json(&v))
    .and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string()))
    .unwrap_or_default()
}
fn epoch() -> chrono::DateTime<chrono::FixedOffset> {
    chrono::DateTime::from_timestamp(0, 0)
        .unwrap()
        .fixed_offset()
}
pub(crate) fn call(name: &str, args: Vec<Value>) -> Result<Value, String> {
    standard("itinerary_core", name, args, epoch())
}
fn object<const N: usize>(fields: [(&str, Value); N]) -> Value {
    record(fields.into_iter().map(|(k, v)| (k.into(), v)))
}
fn range(doc: Option<&Document>, span: Span) -> Value {
    doc.map(|d| from_json(&serde_json::json!(span.range(&d.text))))
        .unwrap_or(Value::Null)
}
fn line_fields(doc: Option<&Document>, row: usize) -> Vec<(String, Value)> {
    vec![
        ("line".into(), Value::Count(row)),
        (
            "raw".into(),
            Value::Text(doc.map(|d| d.line(row)).unwrap_or("").into()),
        ),
        (
            "line_range".into(),
            doc.map(|d| {
                from_json(&serde_json::json!(Range::new(
                    Position::new(row as u32, 0),
                    d.line_end(row)
                )))
            })
            .unwrap_or(Value::Null),
        ),
        (
            "anchor".into(),
            doc.map(|d| from_json(&serde_json::json!(d.line_end(row))))
                .unwrap_or(Value::Null),
        ),
    ]
}
fn day_parts(day: &Day) -> Value {
    object([
        ("month", Value::Count(day.month as usize)),
        ("day", Value::Count(day.day as usize)),
        (
            "year",
            day.year
                .map(|y| Value::Number(y as f64))
                .unwrap_or(Value::Null),
        ),
    ])
}
pub(crate) fn day_record(day: &Day, doc: &Document) -> Value {
    let Value::Record(mut fields) = day_parts(day) else {
        unreachable!()
    };
    fields.extend(line_fields(Some(doc), day.line));
    fields.extend([
        (
            "weekday".into(),
            day.weekday
                .map(|(w, _)| Value::Count(w.num_days_from_monday() as usize))
                .unwrap_or(Value::Null),
        ),
        (
            "weekday_range".into(),
            day.weekday
                .map(|(_, span)| range(Some(doc), span))
                .unwrap_or(Value::Null),
        ),
        ("date_range".into(), range(Some(doc), day.date_span)),
        (
            "places".into(),
            day.places
                .as_ref()
                .map(|(p, _)| Value::Text(p.clone()))
                .unwrap_or(Value::Null),
        ),
        ("forecast".into(), Value::Null),
        (
            "stops".into(),
            Value::List(
                day.stops
                    .iter()
                    .map(|s| stop_record(s, Some(doc)))
                    .collect(),
            ),
        ),
    ]);
    Value::Record(fields)
}
pub(crate) fn stop_record(stop: &Stop, doc: Option<&Document>) -> Value {
    record(
        line_fields(doc, stop.line).into_iter().chain([
            (
                "kind".into(),
                stop.kind
                    .map(|k| {
                        object([
                            ("marker", Value::Text(k.marker.to_string())),
                            ("name", Value::Text(k.name.into())),
                        ])
                    })
                    .unwrap_or(Value::Null),
            ),
            (
                "time".into(),
                Value::Duration(stop.time.num_seconds_from_midnight() as i64),
            ),
            ("twelve_hour".into(), Value::Bool(stop.twelve_hour)),
            ("title".into(), Value::Text(stop.title.clone())),
            ("time_range".into(), range(doc, stop.time_span)),
            ("title_range".into(), range(doc, stop.title_span)),
            (
                "range".into(),
                range(
                    doc,
                    Span::new(stop.line, stop.time_span.start, stop.title_span.end),
                ),
            ),
            (
                "details".into(),
                Value::List(
                    stop.details
                        .iter()
                        .map(|d| {
                            record(line_fields(doc, d.line).into_iter().chain([
                                ("key".into(), Value::Text(d.key.clone())),
                                ("value".into(), Value::Text(d.value.clone())),
                            ]))
                        })
                        .collect(),
                ),
            ),
            (
                "notes".into(),
                Value::List(
                    stop.notes
                        .iter()
                        .map(|s| record(line_fields(doc, s.line)))
                        .collect(),
                ),
            ),
        ]),
    )
}
