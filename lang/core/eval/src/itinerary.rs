//! Itinerary resolution: parsed days and stops become module values, and the
//! `itinerary_core` module decides their dates, labels and canonical text.
use crate::engine::Value;
use chrono::{NaiveDate, Timelike};
use common::Span;
use model::{
    Document,
    itinerary::{Day, Stop},
};
use modules::ModuleRegistry;
use values::EvalResult;
use values::{ToValue, geometry, record, words};

// An itinerary's shape is parsed in `model`; this module resolves it, and
// both halves answer to `eval::itinerary`.

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
    Ok(values::list(&result)?
        .iter()
        .map(|v| match v {
            Value::Date(d) => Some(*d),
            _ => None,
        })
        .collect())
}
pub fn display_time(modules: &ModuleRegistry, stop: &Stop) -> String {
    stop_words(modules, "time_text", stop)
}
pub fn label(modules: &ModuleRegistry, stop: &Stop) -> String {
    stop_words(modules, "label", stop)
}
fn stop_words(modules: &ModuleRegistry, hook: &str, stop: &Stop) -> String {
    words(call(modules, hook, vec![stop_record(stop, None)]))
}
pub(crate) fn call(modules: &ModuleRegistry, name: &str, args: Vec<Value>) -> EvalResult<Value> {
    modules.call(
        "itinerary_core",
        name,
        args,
        chrono::DateTime::UNIX_EPOCH.fixed_offset(),
    )
}
fn range(doc: Option<&Document>, span: Span) -> Value {
    doc.map(|d| geometry(span.range(&d.text)))
        .unwrap_or(Value::Null)
}
fn line_record(doc: Option<&Document>, row: usize) -> LineRecord {
    LineRecord {
        line: row,
        raw: doc.map(|d| d.line(row)).unwrap_or("").into(),
        line_range: doc
            .map(|d| geometry(Span::new(row, 0, d.line(row).len()).range(&d.text)))
            .unwrap_or(Value::Null),
        anchor: doc
            .map(|d| geometry(d.line_end(row)))
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
record! {
    /// Where a line of an itinerary sits, for every record that names one.
    pub(crate) struct LineRecord {
        pub line: usize,
        pub raw: String,
        pub line_range: Value,
        pub anchor: Value,
    }
}

record! {
    /// The calendar parts a day line spells out, before any year is carried
    /// forward.
    pub(crate) struct DayParts {
        pub month: usize,
        pub day: usize,
        pub year: Option<f64>,
    }
}

record! {
    /// What kind of stop a marker names.
    pub(crate) struct KindRecord {
        pub marker: String,
        pub name: String,
    }
}

record! {
    /// One `key: value` line hanging off a stop.
    pub(crate) struct DetailRecord {
        ..line: LineRecord,
        pub key: String,
        pub value: String,
    }
}

record! {
    /// One stop of an itinerary day, with every span the editor draws on.
    pub(crate) struct StopRecord {
        ..line: LineRecord,
        pub kind: Option<KindRecord>,
        pub time: i64,
        pub twelve_hour: bool,
        pub title: String,
        pub time_range: Value,
        pub title_range: Value,
        pub range: Value,
        pub details: Vec<DetailRecord>,
        pub notes: Vec<LineRecord>,
    }
}

record! {
    /// One day of an itinerary: its calendar parts, its line, and its stops.
    pub(crate) struct DayRecord {
        ..parts: DayParts,
        ..line: LineRecord,
        pub weekday: Option<usize>,
        pub weekday_range: Value,
        pub date_range: Value,
        pub places: Option<String>,
        pub forecast: Value,
        pub stops: Vec<StopRecord>,
    }
}
