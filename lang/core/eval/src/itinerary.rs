//! Itinerary resolution: parsed days and stops become module values, and the
//! `itinerary_core` module decides their dates, labels and canonical text.
use crate::contract::itinerary_core;
use crate::engine::Value;
use crate::stdlib;
use chrono::{NaiveDate, Timelike};
use common::Span;
use model::{
    Document,
    itinerary::{Day, Stop},
};
use modules::ModuleRegistry;
use values::EvalResult;
use values::{ToValue, geometry, record};

// An itinerary's shape is parsed in `model`; this module resolves it, and
// both halves answer to `eval::itinerary`.

/// Calendar dates for each day. Years carry forward from the previous day or
/// an explicit year, and a first day without one is the next occurrence.
///
/// `itinerary_core.dates` decides these, so a failure is the caller's to
/// report: the note's diagnostics carry it on its first day.
pub fn dates(
    modules: &ModuleRegistry,
    days: &[Day],
    today: NaiveDate,
) -> EvalResult<Vec<Option<NaiveDate>>> {
    let days = days.iter().map(|d| day_parts(d).to_value()).collect();
    itinerary_core::dates(&mut snapshot(modules), days, today)
}
/// A stop's time as its label writes it.
pub fn display_time(modules: &ModuleRegistry, stop: &Stop) -> stdlib::Presented {
    itinerary_core::time_text(&mut snapshot(modules), stop_record(stop, None))
}
/// A stop's inline label.
pub fn label(modules: &ModuleRegistry, stop: &Stop) -> stdlib::Presented {
    itinerary_core::label(&mut snapshot(modules), stop_record(stop, None))
}
/// The registry as `itinerary_core` sees it: without a clock, since the
/// contract calls it with every date it needs (`today` for `dates`).
fn snapshot(modules: &ModuleRegistry) -> stdlib::Snapshot<'_> {
    stdlib::Snapshot::clockless(modules)
}
fn range(doc: Option<&Document>, span: Span) -> Value {
    doc.map(|d| geometry(span.range(d))).unwrap_or(Value::Null)
}
fn line_record(doc: Option<&Document>, row: usize) -> LineRecord {
    LineRecord {
        line: row,
        raw: doc.map(|d| d.line(row)).unwrap_or("").into(),
        line_range: doc
            .map(|d| geometry(Span::new(row, 0, d.line(row).len()).range(d)))
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

#[cfg(test)]
mod tests {
    /// `itinerary_core` words the warning for a stop without a kind itself, so
    /// it names the kinds again: exactly those in `KINDS`, in their order.
    #[test]
    fn the_missing_kind_warning_names_every_kind() {
        let source = include_str!("../../../stdlib/itinerary_core.xmd");
        let kinds = model::itinerary::KINDS;
        let markers: Vec<String> = kinds.iter().map(|k| k.marker.to_string()).collect();
        let names: Vec<String> = kinds.iter().map(|k| k.name.to_lowercase()).collect();
        let expected = format!(
            "start the title with one of {} ({})",
            markers.join(" "),
            names.join(", ")
        );
        assert!(
            source.contains(&expected),
            "itinerary_core.xmd should say: {expected}"
        );
    }
}
