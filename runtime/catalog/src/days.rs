//! Day records: one per itinerary day, with its forecast when one is cached.
use super::value as q;
use super::{
    RecordKind,
    record::{Base, Record},
};
use lang::eval::Workspace;
use lang::eval::engine::{Engine, Value};
use lang::eval::record;
use lang::model::Document;
use std::path::Path;

record! {
    #[derive(Clone, Debug)]
    pub(super) struct DayRecord {
        ..base: Base,
        value: Value,
    }
}

pub(super) fn days(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    engine: &mut Engine<'_>,
    records: &mut Vec<Record>,
) {
    // A date only finds a day's forecast, so a day that cannot be dated keeps
    // its record and carries the reason, like its stops; the note's
    // diagnostics report it once, on the itinerary.
    let (dates, error) = match lang::eval::itinerary::dates(ws.modules(), &doc.days, engine.today())
    {
        Ok(dates) => (dates, None),
        Err(e) => (vec![None; doc.days.len()], Some(e.to_string())),
    };
    for (day, date) in doc.days.iter().zip(dates) {
        let mut value = lang::eval::itinerary::day_record(day, doc);
        if let Some(date) = date
            && let Some((places, _)) = &day.places
            && let Some(place) = lang::eval::lookups::day_place(places)
            && let Some(lookup) =
                lang::eval::lookups::LookupKey::forecast(&place, date).lookup(ws.lookups())
            && let Value::Record(fields) = &mut value
        {
            fields.insert(
                "forecast".into(),
                lang::eval::modules::record([
                    ("place", Value::Text(place)),
                    (
                        "display",
                        Value::Text(
                            lang::eval::lookups::forecast_from(&lookup.value, false)
                                .map(|f| f.display())
                                .unwrap_or_else(|e| e.to_string()),
                        ),
                    ),
                    ("source", Value::Text(lookup.source.clone())),
                    (
                        "fetched_at",
                        Value::DateTime(lookup.fetched_at.fixed_offset()),
                    ),
                ]),
            );
        }
        let mut base = Base::line(ws, path, RecordKind::Day, doc.line(day.line), day.line);
        base.errors.extend(error.clone());
        records.push(Record::typed(
            path,
            DayRecord {
                base,
                value: q::query_value(value),
            },
        ));
    }
}
