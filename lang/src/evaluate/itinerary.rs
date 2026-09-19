//! Itinerary resolution: parsed days and stops become module values, and the
//! `itinerary_core` module decides their dates, labels and canonical text.
use crate::document::{Document, Span};
use crate::model::itinerary::{Day, Stop};
use crate::{
    engine::Value,
    modules::{ModuleRegistry, from_json, record},
};
use chrono::{NaiveDate, Timelike};
use lsp_types::{Position, Range};

/// Calendar dates for each day. Years carry forward from the previous day or
/// an explicit year, and a first day without one is the next occurrence.
pub fn dates(modules: &ModuleRegistry, days: &[Day], today: NaiveDate) -> Vec<Option<NaiveDate>> {
    try_dates(modules, days, today).unwrap_or_else(|_| vec![None; days.len()])
}
pub(crate) fn try_dates(
    modules: &ModuleRegistry,
    days: &[Day],
    today: NaiveDate,
) -> Result<Vec<Option<NaiveDate>>, String> {
    let input = Value::List(days.iter().map(day_parts).collect());
    let result = call(modules, "dates", vec![input, Value::Date(today)])?;
    Ok(crate::modules::list(&result)?
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
        .unwrap_or_else(|e| e)
}
pub fn canonical_line(modules: &ModuleRegistry, stop: &Stop) -> String {
    call(modules, "canonical", vec![stop_record(stop, None)])
        .map(|v| v.display())
        .unwrap_or_else(|e| e)
}
pub fn label(modules: &ModuleRegistry, stop: &Stop) -> String {
    call(modules, "label", vec![stop_record(stop, None)])
        .map(|v| v.display())
        .unwrap_or_else(|e| e)
}
fn epoch() -> chrono::DateTime<chrono::FixedOffset> {
    chrono::DateTime::from_timestamp(0, 0)
        .unwrap()
        .fixed_offset()
}
pub(crate) fn call(
    modules: &ModuleRegistry,
    name: &str,
    args: Vec<Value>,
) -> Result<Value, String> {
    modules.call("itinerary_core", name, args, epoch())
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
