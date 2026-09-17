use chrono::{DateTime, FixedOffset, NaiveDate};
use jot::{
    actions, cli, diagnostics, document::Document, intelligence, itinerary, presentation, symbols,
    tables, workspace::Workspace,
};
use std::{collections::BTreeMap, path::Path};
use tower_lsp::lsp_types::*;

fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-09-16T14:00:00-04:00").unwrap()
}
fn today() -> NaiveDate {
    now().date_naive()
}
fn path() -> &'static Path {
    Path::new("/notes/trip.jot")
}
fn note(source: &str) -> Workspace {
    Workspace {
        roots: vec!["/notes".into()],
        documents: [(path().to_path_buf(), Document::parse(source.into()))].into(),
        cache: BTreeMap::new(),
    }
}
fn messages(ws: &Workspace) -> Vec<String> {
    diagnostics::collect(ws, path(), today(), now(), false)
        .into_iter()
        .filter(|d| d.severity != Some(DiagnosticSeverity::WARNING))
        .map(|d| d.message)
        .collect()
}

const TRIP: &str = "\
# New York | Oaxaca

## Friday, November 20, 2026 · New York | Oaxaca

07:04 AM    🛫 Depart JFK for MEX on AM 405
        Reservation Number: LPSNKQ
        Seats: 22B, 22C
Note: Flight duration is 5h 51m. 1 free carry on.

11:55 AM    🛬 Arrive at MEX
        Note: 2h 50m layover.

2:45 PM    🛫 Depart MEX for OAX on AM 1050

06:00 PM    🛏️ Check in to Majagua Hotel
Address: 523 Calle de José María Pino Suárez, Centro, Oaxaca, Mexico, 68000
        Confirmation Number: EXP-2550163618

07:20 PM    🍽️ 🌟Dinner at Levadura de Olla
        Cancel by: 24h before
        Note: Chef's tasting menu.

SATURDAY, NOVEMBER 21  OAXACA

11:30 AM    🔍🗺️ Explore Oaxaca
Museum of Cultures of Oaxaca
Zocalo

01:00 PM    🍽️ Grab food at Mercado 20 Noviembre
";

#[test]
fn days_stops_and_details_parse_from_natural_text() {
    let ws = note(TRIP);
    let doc = &ws.documents[path()];
    assert_eq!(doc.days.len(), 2);
    let friday = &doc.days[0];
    assert_eq!(
        (friday.month, friday.day, friday.year),
        (11, 20, Some(2026))
    );
    assert_eq!(
        friday.weekday.as_ref().map(|(w, _)| *w),
        Some(chrono::Weekday::Fri)
    );
    assert_eq!(
        friday.places.as_ref().map(|(p, _)| p.as_str()),
        Some("New York | Oaxaca")
    );
    assert_eq!(friday.stops.len(), 5);
    let flight = &friday.stops[0];
    assert_eq!(flight.title, "Depart JFK for MEX on AM 405");
    assert_eq!(flight.kind.map(|k| k.marker), Some('>'));
    assert!(flight.inferred && flight.marker_span.is_none());
    assert_eq!(friday.stops[3].kind.map(|k| k.name), Some("Stay"));
    assert_eq!(kinds(&doc.days[1]), ["Explore", "Meal"]);
    assert_eq!(itinerary::display_time(flight), "07:04 AM");
    assert_eq!(
        flight
            .details
            .iter()
            .map(|d| (d.key.as_str(), d.value.as_str()))
            .collect::<Vec<_>>(),
        [
            ("Reservation Number", "LPSNKQ"),
            ("Seats", "22B, 22C"),
            ("Note", "Flight duration is 5h 51m. 1 free carry on.")
        ]
    );
    assert_eq!(flight.end_line, 8);
    assert_eq!(itinerary::display_time(&friday.stops[2]), "02:45 PM");
    let saturday = &doc.days[1];
    assert_eq!(saturday.year, None);
    assert_eq!(
        saturday.places.as_ref().map(|(p, _)| p.as_str()),
        Some("OAXACA")
    );
    assert_eq!(saturday.stops[0].notes.len(), 2);
    assert_eq!(
        itinerary::dates(&doc.days, today()),
        [
            NaiveDate::from_ymd_opt(2026, 11, 20),
            NaiveDate::from_ymd_opt(2026, 11, 21)
        ]
    );
    assert_eq!(messages(&ws), Vec::<String>::new());
    // Addresses become map links, so the existing Open code lens works.
    let map = doc
        .links
        .iter()
        .find(|l| l.target.contains("google.com/maps"))
        .unwrap();
    assert!(
        map.target.contains("query=523+Calle+de+Jos%C3%A9"),
        "{}",
        map.target
    );
}

fn kinds(day: &itinerary::Day) -> Vec<&'static str> {
    day.stops
        .iter()
        .map(|s| s.kind.map(|k| k.name).unwrap_or("?"))
        .collect()
}

#[test]
fn markers_are_parsed_and_unknown_kinds_are_warned_about() {
    let ws = note(
        "Friday, November 20, 2026\n\n09:00 AM  > JFK to MEX\n10:00 AM  Something vague\n11:00 AM  *Lunch\n",
    );
    let stops = &ws.documents[path()].days[0].stops;
    assert_eq!(stops[0].kind.map(|k| k.name), Some("Depart"));
    assert!(!stops[0].inferred && stops[0].marker_span.is_some());
    assert_eq!(stops[0].title, "JFK to MEX");
    assert_eq!(stops[1].kind, None);
    // A marker needs a space after it; "*Lunch" is a title starting with an asterisk.
    assert_eq!(stops[2].kind.map(|k| k.name), None);
    let warnings: Vec<_> = diagnostics::collect(&ws, path(), today(), now(), false)
        .into_iter()
        .filter(|d| d.severity == Some(DiagnosticSeverity::WARNING))
        .map(|d| d.range.start.line)
        .collect();
    assert_eq!(warnings, [3, 4]);
    assert_eq!(
        itinerary::canonical_line(&stops[0]),
        "09:00 AM  > JFK to MEX"
    );
    assert_eq!(itinerary::label(&stops[1]), "Something vague");
}

#[test]
fn years_carry_forward_and_a_bare_first_day_is_upcoming() {
    let ws = note("December 30\n\n9:00 AM Fly\n\nJanuary 2\n\n9:00 AM Fly home\n");
    let dates = itinerary::dates(&ws.documents[path()].days, today());
    assert_eq!(
        dates,
        [
            NaiveDate::from_ymd_opt(2026, 12, 30),
            NaiveDate::from_ymd_opt(2027, 1, 2)
        ]
    );
    let past = note("March 3\n\n9:00 AM Something\n");
    assert_eq!(
        itinerary::dates(&past.documents[path()].days, today()),
        [NaiveDate::from_ymd_opt(2027, 3, 3)]
    );
    // Prose that merely mentions a date is not a day heading.
    let prose = note("On Friday, November 20th we leave.\nNovember is busy.\n");
    assert!(prose.documents[path()].days.is_empty());
}

#[test]
fn itinerary_diagnostics_catch_wrong_weekdays_order_and_bad_dates() {
    let ws = note(
        "Thursday, November 20, 2026\n\n09:00 AM Coffee\n08:00 AM Earlier\n\nFebruary 30\n\n1:00 PM Nothing\n\nNovember 19, 2026\n",
    );
    let issues = messages(&ws);
    assert_eq!(
        issues,
        [
            "November 20, 2026 is a Friday, not a Thursday",
            "08:00 AM is earlier than the previous stop at 09:00 AM",
            "February 30 is not a valid date",
            "2026-11-19 comes before the previous day, 2026-11-20",
        ]
    );
}

#[test]
fn format_document_normalizes_times_and_detail_indentation() {
    let doc = Document::parse(TRIP.into());
    let formatted = actions::apply_edits(TRIP, &tables::formatting(&doc)).unwrap();
    assert!(
        formatted.contains("02:45 PM  > Depart MEX for OAX on AM 1050\n"),
        "{formatted}"
    );
    assert!(formatted.contains("07:04 AM  > Depart JFK for MEX on AM 405\n    Reservation Number: LPSNKQ\n    Seats: 22B, 22C\n    Note: Flight duration is 5h 51m. 1 free carry on.\n"), "{formatted}");
    assert!(
        formatted
            .contains("11:30 AM  ? Explore Oaxaca\n    Museum of Cultures of Oaxaca\n    Zocalo\n"),
        "{formatted}"
    );
    // Headings and prose are untouched, and formatting is idempotent.
    assert!(formatted.contains("## Friday, November 20, 2026 · New York | Oaxaca\n"));
    assert!(formatted.contains("SATURDAY, NOVEMBER 21  OAXACA\n"));
    let again = Document::parse(formatted.clone());
    assert!(tables::formatting(&again).is_empty());
}

#[test]
fn inlays_hover_outline_and_folding_describe_the_trip() {
    let ws = note(TRIP);
    let doc = &ws.documents[path()];
    let hints = presentation::hints_at(
        &ws,
        path(),
        now(),
        Range::new(Position::new(0, 0), Position::new(40, 0)),
    );
    let label = |line: u32| {
        hints
            .iter()
            .find(|h| h.position.line == line)
            .map(|h| match &h.label {
                InlayHintLabel::String(s) => s.clone(),
                other => panic!("{other:?}"),
            })
            .unwrap_or_default()
    };
    assert_eq!(label(2), "5 stops · 07:04 AM – 07:20 PM · in 65 days");
    assert_eq!(label(4), "4h 51m until Arrive at MEX");
    assert_eq!(label(9), "2h 50m layover");
    assert_eq!(label(18), "cancel by Thu Nov 19, 07:20 PM");
    assert_eq!(label(22), "2 stops · 11:30 AM – 01:00 PM · in 66 days");
    let hover = intelligence::stop_hover(&ws, path(), Position::new(9, 3), today()).unwrap();
    let text = match hover.contents {
        HoverContents::Markup(m) => m.value,
        other => panic!("{other:?}"),
    };
    assert!(
        text.starts_with(
            "**Arrive at MEX**\n\nArrive · 11:55 AM, Friday, November 20, 2026\n\n2h 50m until"
        ),
        "{text}"
    );
    assert!(text.contains("**Note:** 2h 50m layover."), "{text}");
    let outline = symbols::document_symbols(&ws, path(), now());
    let trip = &outline[0];
    let friday = trip
        .children
        .as_ref()
        .unwrap()
        .iter()
        .find(|s| s.name.starts_with("Friday, November 20"))
        .unwrap();
    assert_eq!(friday.name, "Friday, November 20, 2026 · New York | Oaxaca");
    assert_eq!(
        friday.detail.as_deref(),
        Some("5 stops · Friday 2026-11-20")
    );
    let stops: Vec<_> = friday
        .children
        .as_ref()
        .unwrap()
        .iter()
        .map(|s| s.detail.clone().unwrap())
        .collect();
    assert_eq!(
        stops,
        [
            "07:04 AM · Depart",
            "11:55 AM · Arrive",
            "02:45 PM · Depart",
            "06:00 PM · Stay",
            "07:20 PM · Meal"
        ]
    );
    let saturday = trip
        .children
        .as_ref()
        .unwrap()
        .iter()
        .find(|s| s.name.starts_with("November 21"))
        .unwrap();
    assert_eq!(
        saturday.detail.as_deref(),
        Some("2 stops · Saturday 2026-11-21")
    );
    let folds = symbols::folding_ranges(doc);
    assert!(
        folds.iter().any(|f| f.start_line == 2 && f.end_line == 20),
        "{folds:?}"
    );
    assert!(
        folds.iter().any(|f| f.start_line == 4 && f.end_line == 7),
        "{folds:?}"
    );
    assert!(
        folds.iter().any(|f| f.start_line == 22 && f.end_line == 28),
        "{folds:?}"
    );
}

#[test]
fn completion_offers_stop_kinds_after_a_time_and_keys_inside_a_stop() {
    let ws = note("Friday, November 20, 2026\n\n09:00 AM De\n\n10:00 AM Coffee\nAd\n");
    let kinds = intelligence::completions(&ws, path(), Position::new(2, 11), now(), false);
    assert_eq!(
        kinds.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
        ["> Depart"]
    );
    let keys = intelligence::completions(&ws, path(), Position::new(5, 2), now(), false);
    assert_eq!(
        keys.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
        ["Address:"]
    );
}

#[test]
fn stops_join_the_agenda_in_time_order() {
    let ws = note(TRIP);
    let entries: Vec<_> = cli::entries(&ws, today())
        .into_iter()
        .filter(|e| e.kind == "stop")
        .collect();
    assert_eq!(entries.len(), 7);
    assert_eq!(entries[0].at.as_deref(), Some("2026-11-20 07:04"));
    assert_eq!(entries[0].at_date, NaiveDate::from_ymd_opt(2026, 11, 20));
    assert!(cli::agenda_entry(
        &entries[0],
        NaiveDate::from_ymd_opt(2026, 11, 20).unwrap(),
        NaiveDate::from_ymd_opt(2026, 11, 26).unwrap()
    ));
    assert!(!cli::agenda_entry(&entries[0], today(), today()));
}
