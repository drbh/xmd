//! Day records: one per itinerary day, with its forecast when one is cached.
use super::{
    QueryValue, RecordKind,
    record::{Base, Fields, Record, entries},
};
use crate::{
    document::Document,
    engine::{Engine, Value},
    workspace::Workspace,
};
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Debug)]
struct DayRecord {
    base: Base,
    value: QueryValue,
}
impl Fields for DayRecord {
    fn fields(self) -> BTreeMap<String, QueryValue> {
        let mut fields = self.base.fields();
        fields.extend(entries([("value", self.value)]));
        fields
    }
}

pub(super) fn days(
    ws: &Workspace,
    path: &Path,
    doc: &Document,
    engine: &mut Engine<'_>,
    records: &mut Vec<Record>,
) -> Result<(), String> {
    let dates = crate::itinerary::try_dates(&ws.modules, &doc.days, engine.today)
        .map_err(|e| e.to_string())?;
    for (day, date) in doc.days.iter().zip(dates) {
        let mut value = crate::itinerary::day_record(day, doc);
        if let Some(date) = date
            && let Some((places, _)) = &day.places
            && let Some(place) = crate::lookups::day_place(places)
            && let Some(lookup) =
                crate::lookups::LookupKey::forecast(&place, date).lookup(&ws.lookups)
            && let Value::Record(fields) = &mut value
        {
            fields.insert(
                "forecast".into(),
                crate::modules::record([
                    ("place".into(), Value::Text(place)),
                    (
                        "display".into(),
                        Value::Text(
                            crate::lookups::forecast_from(&lookup.value, false)
                                .map(|f| f.display())
                                .unwrap_or_else(|e| e.to_string()),
                        ),
                    ),
                    ("source".into(), Value::Text(lookup.source.clone())),
                    (
                        "fetched_at".into(),
                        Value::DateTime(lookup.fetched_at.fixed_offset()),
                    ),
                ]),
            );
        }
        records.push(Record::typed(
            path,
            DayRecord {
                base: Base::new(ws, path, day.line, RecordKind::Day, doc.line(day.line)),
                value: QueryValue::from_value(value),
            },
        ));
    }
    Ok(())
}
