//! Trip itineraries: day headings, timed stops, and their details, parsed from
//! prose that reads naturally on its own. Shared by the native server,
//! browser adapter and CLI.
use crate::document::{Document, Span, utf16};
use chrono::{Datelike, Duration, NaiveDate, NaiveTime, Weekday};
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
/// Stop kinds offered by completion after a time; the emoji is the convention.
pub const KINDS: &[(&str, &str)] = &[
    ("🛫", "Depart"),
    ("🛬", "Arrive at"),
    ("🚕", "Transit"),
    ("🚶", "Walk to"),
    ("🚆", "Train to"),
    ("🛏️", "Check in to"),
    ("🧳", "Check out of"),
    ("🍽️", "Dinner at"),
    ("☕", "Breakfast at"),
    ("🌳", "Visit"),
    ("🔍", "Explore"),
    ("🎟️", "Tour"),
];
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
            let title = line[title_start..].trim_end();
            day.stops.push(Stop {
                line: row,
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
    let mut previous: Option<NaiveDate> = None;
    let mut year = days.iter().find_map(|d| d.year).unwrap_or(today.year());
    days.iter()
        .map(|day| {
            if let Some(y) = day.year {
                year = y;
            }
            let mut date = NaiveDate::from_ymd_opt(year, day.month, day.day);
            if day.year.is_none() {
                if let Some(prev) = previous
                    && let Some(d) = date
                    && d < prev
                {
                    year += 1;
                    date = NaiveDate::from_ymd_opt(year, day.month, day.day);
                } else if previous.is_none()
                    && let Some(d) = date
                    && d < today - Duration::days(30)
                {
                    year += 1;
                    date = NaiveDate::from_ymd_opt(year, day.month, day.day);
                }
            }
            if let Some(d) = date {
                previous = Some(d);
            }
            date
        })
        .collect()
}
pub fn display_time(stop: &Stop) -> String {
    if stop.twelve_hour {
        stop.time.format("%I:%M %p").to_string()
    } else {
        stop.time.format("%H:%M").to_string()
    }
}
/// `4h 51m`, `2h`, or `45m`: the way a traveler says it.
pub fn human(seconds: i64) -> String {
    let minutes = seconds / 60;
    match (minutes / 60, minutes % 60) {
        (0, m) => format!("{m}m"),
        (h, 0) => format!("{h}h"),
        (h, m) => format!("{h}h {m}m"),
    }
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
    let seconds = (next.time - stop.time).num_seconds();
    (seconds >= 0).then_some(seconds)
}
/// A Google Maps search for an address; every platform opens it.
pub fn map_url(address: &str) -> String {
    let encoded: String = url::form_urlencoded::byte_serialize(address.as_bytes()).collect();
    format!("https://www.google.com/maps/search/?api=1&query={encoded}")
}
/// `Cancel by: 24h before` or an explicit datetime, resolved against the stop.
pub fn cancel_by(day: NaiveDate, stop: &Stop) -> Option<(chrono::NaiveDateTime, bool)> {
    let detail = stop
        .details
        .iter()
        .find(|d| d.key.eq_ignore_ascii_case("cancel by"))?;
    let value = detail.value.trim();
    if let Some(offset) = value
        .strip_suffix("before")
        .or_else(|| value.strip_suffix("in advance"))
        .or_else(|| value.strip_suffix("ahead"))
    {
        let seconds = crate::engine::duration(offset.trim())?;
        return Some((day.and_time(stop.time) - Duration::seconds(seconds), true));
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M") {
        return Some((dt, false));
    }
    if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M") {
        return Some((dt, false));
    }
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d").ok()?;
    Some((date.and_hms_opt(23, 59, 0)?, false))
}

/// Normalize stop lines to `07:04 AM    Title` and indent details by four.
pub fn formatting(doc: &Document, days: &[Day]) -> Vec<TextEdit> {
    let mut edits = Vec::new();
    let mut replace = |row: usize, text: String| {
        let line = doc.line(row);
        if line != text {
            edits.push(TextEdit::new(
                Range::new(
                    Position::new(row as u32, 0),
                    Position::new(row as u32, utf16(line, line.len())),
                ),
                text,
            ));
        }
    };
    for day in days {
        for stop in &day.stops {
            replace(
                stop.line,
                format!("{}    {}", display_time(stop), stop.title),
            );
            for detail in &stop.details {
                replace(detail.line, format!("    {}: {}", detail.key, detail.value));
            }
            for note in &stop.notes {
                let text = doc.line(note.line)[note.start..note.end].trim().to_string();
                replace(note.line, format!("    {text}"));
            }
        }
    }
    edits
}
