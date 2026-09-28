//! Presentation-only recognition: these values do not create symbols or tasks.
//! Match complete words, validate dates/times, and leave malformed values plain.
use chrono::{DateTime, NaiveDate, NaiveDateTime};
use eval::engine::{self, Lexeme, Literal};

struct Word<'a> {
    start: usize,
    end: usize,
    text: &'a str,
}

pub(super) fn values(line: &str) -> Vec<(usize, usize, &'static str)> {
    let mut words = Vec::new();
    let mut offset = 0;
    for raw in line.split_inclusive(char::is_whitespace) {
        let text = raw
            .trim()
            .trim_start_matches(['(', '[', '{', '"', '\'', '“', '‘'])
            .trim_end_matches([
                '.', ',', ';', ':', '!', '?', ')', ']', '}', '"', '\'', '”', '’',
            ]);
        if !text.is_empty() {
            let start = offset + raw.find(text).unwrap();
            words.push(Word {
                start,
                end: start + text.len(),
                text,
            });
        }
        offset += raw.len();
    }
    let mut result = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let word = &words[i];
        // A time's AM/PM suffix and "next Monday" belong to one visual token.
        if let Some(next) = words.get(i + 1)
            && line[word.end..next.start].chars().all(char::is_whitespace)
        {
            let combined = format!("{} {}", word.text, next.text);
            let kind = if clock_time(&combined) {
                Some("wtfTime")
            } else if word.text.eq_ignore_ascii_case("next") && relative_date(&combined) {
                Some("wtfDate")
            } else {
                None
            };
            if let Some(kind) = kind {
                result.push((word.start, next.end, kind));
                i += 2;
                continue;
            }
        }
        if let Some(kind) = kind(word.text) {
            result.push((word.start, word.end, kind));
        }
        i += 1;
    }
    result
}

fn relative_date(text: &str) -> bool {
    engine::relative_date(text, NaiveDate::from_ymd_opt(2000, 1, 1).unwrap()).is_some()
}
fn calendar_date(text: &str) -> bool {
    // Slash dates in prose use month/day/four-digit-year, matching common notes.
    // This does not extend the evaluator's deliberately ISO-only date syntax.
    let parts: Vec<_> = text.split('/').collect();
    if parts.len() == 3
        && parts[2].len() == 4
        && parts[..2].iter().all(|s| !s.is_empty() && s.len() <= 2)
        && parts.iter().all(|s| s.bytes().all(|b| b.is_ascii_digit()))
    {
        return NaiveDate::parse_from_str(text, "%m/%d/%Y").is_ok();
    }
    if text.as_bytes().get(4) != Some(&b'-') {
        return false;
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d").is_ok()
        || DateTime::parse_from_rfc3339(text).is_ok()
        || DateTime::parse_from_str(text, "%Y-%m-%dT%H:%M%:z").is_ok()
        // Highlight local timestamps independently of host timezone/DST rules.
        || NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M").is_ok()
}
fn clock_time(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let (body, meridiem) = lower
        .strip_suffix("am")
        .or_else(|| lower.strip_suffix("pm"))
        .map(|body| (body.trim_end(), true))
        .unwrap_or((&lower, false));
    let parts: Vec<_> = body.split(':').collect();
    if parts.len() > 3 || !meridiem && parts.len() < 2 || parts[0].is_empty() || parts[0].len() > 2
    {
        return false;
    }
    if !parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit())) {
        return false;
    }
    let Ok(hour) = parts[0].parse::<u32>() else {
        return false;
    };
    if meridiem && !(1..=12).contains(&hour) || !meridiem && hour > 23 {
        return false;
    }
    parts[1..]
        .iter()
        .all(|part| part.len() == 2 && part.parse::<u32>().is_ok_and(|n| n < 60))
}
fn kind(text: &str) -> Option<&'static str> {
    if relative_date(text) {
        return Some("wtfDate");
    }
    if matches!(text, "true" | "false") {
        return Some("wtfBoolean");
    }
    let first = text.chars().next()?;
    if !first.is_ascii_digit() && !matches!(first, '$' | '-' | '+' | '.') {
        return None;
    }
    if calendar_date(text) {
        return Some("wtfDate");
    }
    if clock_time(text) {
        return Some("wtfTime");
    }
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    let tokens = engine::lex(unsigned).ok()?;
    let [token] = tokens.as_slice() else {
        return None;
    };
    if token.start != 0 || token.end != unsigned.len() {
        return None;
    }
    match token.kind {
        Lexeme::Value(Literal::Money(..)) => Some("wtfMoney"),
        Lexeme::Value(Literal::Ratio(_)) => Some("wtfRatio"),
        Lexeme::Value(Literal::Duration(_)) => Some("wtfDuration"),
        Lexeme::Value(Literal::Number(_)) => Some("number"),
        _ => None,
    }
}
