//! Itinerary resolution: parsed days and stops become module values, and the
//! `itinerary_core` module decides their dates, labels and canonical text.
use crate::error::EvalResult;
use crate::{
    engine_impl::Value,
    modules_impl::{ModuleRegistry, from_json},
    records::{DayParts, DayRecord, DetailRecord, KindRecord, LineRecord, StopRecord, ToValue},
};
use chrono::{NaiveDate, Timelike};
use common::Span;
use lsp_types::{Position, Range};
use model::Document;

// An itinerary's shape is parsed in `model`; this module resolves it, and
// both halves answer to `eval::itinerary`.
pub use model::itinerary::*;

/// Calendar dates for each day. Years carry forward from the previous day or
/// an explicit year, and a first day without one is the next occurrence.
pub fn dates(modules: &ModuleRegistry, days: &[Day], today: NaiveDate) -> Vec<Option<NaiveDate>> {
    try_dates(modules, days, today).unwrap_or_else(|_| vec![None; days.len()])
}
pub fn try_dates(
    modules: &ModuleRegistry,
    days: &[Day],
    today: NaiveDate,
) -> EvalResult<Vec<Option<NaiveDate>>> {
    let input = Value::List(days.iter().map(|d| day_parts(d).to_value()).collect());
    let result = call(modules, "dates", vec![input, Value::Date(today)])?;
    Ok(crate::modules_impl::list(&result)?
        .iter()
        .map(|v| match v {
            Value::Date(d) => Some(*d),
            _ => None,
        })
        .collect())
}
pub fn display_time(modules: &ModuleRegistry, stop: &Stop) -> String {
    call(modules, "time_text", vec![stop_record(stop, None)])
        .map(|v| v.display())
        .unwrap_or_else(|e| e.to_string())
}
pub fn label(modules: &ModuleRegistry, stop: &Stop) -> String {
    call(modules, "label", vec![stop_record(stop, None)])
        .map(|v| v.display())
        .unwrap_or_else(|e| e.to_string())
}
fn epoch() -> chrono::DateTime<chrono::FixedOffset> {
    chrono::DateTime::from_timestamp(0, 0)
        .unwrap()
        .fixed_offset()
}
pub(crate) fn call(modules: &ModuleRegistry, name: &str, args: Vec<Value>) -> EvalResult<Value> {
    modules.call("itinerary_core", name, args, epoch())
}
fn range(doc: Option<&Document>, span: Span) -> Value {
    doc.map(|d| from_json(&serde_json::json!(span.range(&d.text))))
        .unwrap_or(Value::Null)
}
fn line_record(doc: Option<&Document>, row: usize) -> LineRecord {
    LineRecord {
        line: row,
        raw: doc.map(|d| d.line(row)).unwrap_or("").into(),
        line_range: doc
            .map(|d| {
                from_json(&serde_json::json!(Range::new(
                    Position::new(row as u32, 0),
                    d.line_end(row)
                )))
            })
            .unwrap_or(Value::Null),
        anchor: doc
            .map(|d| from_json(&serde_json::json!(d.line_end(row))))
            .unwrap_or(Value::Null),
    }
}
fn day_parts(day: &Day) -> DayParts {
    DayParts {
        month: day.month as usize,
        day: day.day as usize,
        year: day.year.map(|y| y as f64),
    }
}
pub fn day_record(day: &Day, doc: &Document) -> Value {
    DayRecord {
        parts: day_parts(day),
        line: line_record(Some(doc), day.line),
        weekday: day.weekday.map(|(w, _)| w.num_days_from_monday() as usize),
        weekday_range: day
            .weekday
            .map(|(_, span)| range(Some(doc), span))
            .unwrap_or(Value::Null),
        date_range: range(Some(doc), day.date_span),
        places: day.places.as_ref().map(|(p, _)| p.clone()),
        forecast: Value::Null,
        stops: day.stops.iter().map(|s| stop(s, Some(doc))).collect(),
    }
    .to_value()
}
fn stop(stop: &Stop, doc: Option<&Document>) -> StopRecord {
    StopRecord {
        line: line_record(doc, stop.line),
        kind: stop.kind.map(|k| KindRecord {
            marker: k.marker.to_string(),
            name: k.name.into(),
        }),
        time: stop.time.num_seconds_from_midnight() as i64,
        twelve_hour: stop.twelve_hour,
        title: stop.title.clone(),
        time_range: range(doc, stop.time_span),
        title_range: range(doc, stop.title_span),
        range: range(
            doc,
            Span::new(stop.line, stop.time_span.start, stop.title_span.end),
        ),
        details: stop
            .details
            .iter()
            .map(|d| DetailRecord {
                line: line_record(doc, d.line),
                key: d.key.clone(),
                value: d.value.clone(),
            })
            .collect(),
        notes: stop
            .notes
            .iter()
            .map(|s| line_record(doc, s.line))
            .collect(),
    }
}
pub(crate) fn stop_record(value: &Stop, doc: Option<&Document>) -> Value {
    stop(value, doc).to_value()
}
