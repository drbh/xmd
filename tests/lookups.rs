use chrono::{DateTime, FixedOffset, Utc};
use std::{collections::BTreeMap, path::Path};
use tower_lsp::lsp_types::*;
use wtf::{
    document::Document,
    engine::{Currency, Engine, Value},
    lookups,
    workspace::{Symbol, SymbolKind, Workspace},
};

fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-09-17T14:00:00+00:00").unwrap()
}
fn path() -> &'static Path {
    Path::new("/notes/money.wtf")
}
fn store() -> lookups::Store {
    let fetched: DateTime<Utc> = "2026-09-17T12:00:00Z".parse().unwrap();
    let entry = |value: serde_json::Value, source: &str| lookups::Lookup {
        value,
        fetched_at: fetched,
        source: source.into(),
    };
    [
        ("rate:EUR:USD".to_string(), entry(serde_json::json!({"rate": 1.09}), "frankfurter.app")),
        ("quote:NVDA".to_string(), entry(serde_json::json!({"price": 181.5, "currency": "USD"}), "stooq.com")),
        (
            "forecast:oaxaca:2026-11-20".to_string(),
            entry(serde_json::json!({"high": 24.2, "low": 11.8, "summary": "light rain", "precipitation": 0.4}), "open-meteo.com"),
        ),
        ("forecast:oaxaca:2026-11-21".to_string(), entry(serde_json::json!({"error": "no forecast yet; forecasts cover about 16 days"}), "open-meteo.com")),
    ]
    .into()
}
fn note(source: &str) -> Workspace {
    Workspace {
        roots: vec!["/notes".into()],
        documents: [(path().to_path_buf(), Document::parse(source.into()))].into(),
        cache: BTreeMap::new(),
        lookups: store(),
        modules: Default::default(),
    }
}
fn eval(ws: &Workspace, expression: &str) -> Result<Value, String> {
    Engine::at(ws, now()).eval(path(), expression)
}
fn eur() -> Currency {
    Currency::parse("EUR").unwrap()
}

#[test]
fn money_carries_a_currency_from_symbols_and_codes() {
    let ws = note("");
    assert_eq!(eval(&ws, "€450").unwrap(), Value::Money(450.0, eur()));
    assert_eq!(eval(&ws, "700 MXN").unwrap().display(), "700 MXN");
    assert_eq!(eval(&ws, "-£12.50").unwrap().display(), "-£12.50");
    assert_eq!(eval(&ws, "¥1000 * 2").unwrap().display(), "¥2,000");
    assert_eq!(
        eval(&ws, "$3 + $4").unwrap(),
        Value::Money(7.0, Currency::USD)
    );
    assert_eq!(eval(&ws, "€450 * 3").unwrap().display(), "€1,350");
    assert_eq!(eval(&ws, "€450 / €50").unwrap(), Value::Ratio(9.0));
    assert_eq!(eval(&ws, "€450").unwrap().source().as_deref(), Some("€450"));
    assert_eq!(
        eval(&ws, "700 MXN").unwrap().source().as_deref(),
        Some("700 MXN")
    );
    assert_eq!(
        eval(&ws, "€450 + $5").unwrap_err(),
        "Cannot combine EUR and USD; convert with to(value, USD) (Money + Money)"
    );
    assert!(
        eval(&ws, "€450 > $5")
            .unwrap_err()
            .starts_with("Cannot compare EUR with USD")
    );
    // Durations still lex: 30m is not 30 MXN.
    assert_eq!(eval(&ws, "30m + 2h").unwrap(), Value::Duration(9000));
}

#[test]
fn uppercase_names_are_code_literals_not_references() {
    let ws = note("[cost] := rate(EUR, USD) * 2\n");
    assert_eq!(eval(&ws, "USD").unwrap(), Value::Text("USD".into()));
    assert_eq!(eval(&ws, "NVDA").unwrap(), Value::Text("NVDA".into()));
    assert_eq!(
        wtf::RequestContext::new(&ws, now())
            .diagnostics(path(), false)
            .len(),
        0
    );
    assert!(
        ws.documents[path()]
            .references
            .iter()
            .all(|r| r.name != "EUR" && r.name != "USD")
    );
    assert_eq!(eval(&ws, "cost").unwrap(), Value::Number(2.18));
}

#[test]
fn rates_conversions_quotes_and_forecasts_read_the_cache() {
    let ws = note(
        "[€450]:hotel\n[hotel_usd] := to(hotel, USD)\n[nvda] := quote(NVDA) * 12\n[weather] := forecast(\"Oaxaca\", 2026-11-20, F)\n[rain] := weather.rain\n[later] := forecast(\"Oaxaca\", 2026-11-21)\n[missing] := rate(GBP, USD)\n",
    );
    assert_eq!(eval(&ws, "hotel_usd").unwrap().display(), "$490.50");
    assert_eq!(eval(&ws, "to(hotel, EUR)").unwrap().display(), "€450");
    assert_eq!(eval(&ws, "nvda").unwrap().display(), "$2,178");
    let weather = eval(&ws, "weather").unwrap();
    assert_eq!(weather.display(), "76°F / 53°F · light rain · 40% rain");
    assert_eq!(eval(&ws, "weather.high").unwrap(), Value::Number(76.0));
    assert_eq!(eval(&ws, "rain").unwrap(), Value::Ratio(0.4));
    assert_eq!(
        eval(&ws, "forecast(\"Oaxaca\", 2026-11-20)")
            .unwrap()
            .display(),
        "24°C / 12°C · light rain · 40% rain"
    );
    let messages: Vec<_> = wtf::RequestContext::new(&ws, now())
        .diagnostics(path(), false)
        .into_iter()
        .map(|d| d.message)
        .collect();
    assert_eq!(
        messages,
        [
            "Forecast for Oaxaca on 2026-11-21: no forecast yet; forecasts cover about 16 days",
            "No cached rate GBP→USD; run wtf refresh or use the ⟳ lookups lens",
        ]
    );
    // Hovers show what was read and how old it is; misses say so.
    let hover = wtf::RequestContext::new(&ws, now()).symbol_hover(&Symbol {
        path: path().into(),
        kind: SymbolKind::Definition(1),
    });
    assert!(
        hover.contains("Lookups:\n- rate EUR→USD · 2h ago · frankfurter.app"),
        "{hover}"
    );
    let missing = wtf::RequestContext::new(&ws, now()).symbol_hover(&Symbol {
        path: path().into(),
        kind: SymbolKind::Definition(6),
    });
    assert!(
        missing.contains("- rate GBP→USD · not fetched yet"),
        "{missing}"
    );
}

#[test]
fn itinerary_days_show_weather_and_notes_declare_what_they_want() {
    let ws = note(
        "## Friday, November 20, 2026 · New York | Oaxaca\n\n07:04 AM  > Depart JFK\n\n## Saturday, November 21 · Oaxaca\n\n[fx] := to(€10, USD)\n[stock] := quote(AAPL)\n",
    );
    let hints = wtf::RequestContext::new(&ws, now())
        .hints(
            path(),
            Range::new(Position::new(0, 0), Position::new(20, 0)),
        )
        .hints;
    let label = |line: u32| {
        hints
            .iter()
            .find(|h| h.position.line == line)
            .map(|h| match &h.label {
                InlayHintLabel::String(s) => s.clone(),
                other => panic!("{other:?}"),
            })
            .unwrap()
    };
    assert_eq!(
        label(0),
        "1 stops · 07:04 AM – 07:04 AM · in 64 days · 24°C / 12°C · light rain · 40% rain"
    );
    assert_eq!(
        label(4),
        "0 stops · in 65 days · no forecast yet; forecasts cover about 16 days"
    );
    let wanted: Vec<_> = lookups::wanted(&ws, now().date_naive())
        .into_iter()
        .collect();
    assert_eq!(
        wanted,
        [
            "forecast:oaxaca:2026-11-20",
            "forecast:oaxaca:2026-11-21",
            "quote:AAPL",
            "rate:EUR:USD",
        ]
    );
    assert_eq!(
        lookups::day_place("New York | Oaxaca").as_deref(),
        Some("Oaxaca")
    );
    assert_eq!(
        lookups::describe("forecast:oaxaca:2026-11-20"),
        "forecast oaxaca 2026-11-20"
    );
    assert_eq!(lookups::weather_summary(61), "light rain");
}

#[test]
fn refresh_uses_keyless_providers_or_commands_from_providers_json() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    // A fake curl answers each built-in provider with a canned body.
    std::fs::write(
        bin.join("curl"),
        "#!/bin/sh\nurl=\"$8\"\ncase \"$url\" in\n  *frankfurter*) echo '{\"rates\":{\"USD\":1.09}}' ;;\n  *finance.yahoo*) echo '{\"chart\":{\"result\":[{\"meta\":{\"currency\":\"USD\",\"regularMarketPrice\":181.5}}]}}' ;;\n  *geocoding*) echo '{\"results\":[{\"name\":\"Oaxaca\",\"latitude\":17.06,\"longitude\":-96.72}]}' ;;\n  *2026-11-20*) echo '{\"daily\":{\"temperature_2m_max\":[24.2],\"temperature_2m_min\":[11.8],\"weather_code\":[61],\"precipitation_probability_max\":[40]}}' ;;\n  *) echo 'out of range' >&2; exit 22 ;;\nesac\n",
    )
    .unwrap();
    std::fs::write(
        bin.join("my-quote"),
        "#!/bin/sh\necho \"{\\\"price\\\": 42.5, \\\"currency\\\": \\\"EUR\\\"}\"\n",
    )
    .unwrap();
    for name in ["curl", "my-quote"] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(bin.join(name), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::create_dir_all(root.join(".wtf")).unwrap();
    std::fs::write(
        root.join(".wtf/providers.json"),
        "{\"quote\": \"my-quote {symbol}\"}",
    )
    .unwrap();
    std::fs::write(
        root.join("trip.wtf"),
        "[€450]:hotel\n[usd] := to(hotel, USD)\n[nvda] := quote(NVDA)\n\n## Friday, November 20, 2026 · Oaxaca\n\n## Saturday, December 5, 2026 · Oaxaca\n",
    )
    .unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wtf"))
        .current_dir(&root)
        .env(
            "PATH",
            format!(
                "{}:{}",
                bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .arg("refresh")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let saved: lookups::Store =
        serde_json::from_slice(&std::fs::read(root.join(".wtf/lookups.json")).unwrap()).unwrap();
    assert_eq!(saved["rate:EUR:USD"].value["rate"], 1.09);
    assert_eq!(saved["rate:EUR:USD"].source, "frankfurter.dev");
    assert_eq!(
        saved["quote:NVDA"].value,
        serde_json::json!({"price": 42.5, "currency": "EUR"})
    );
    assert_eq!(saved["quote:NVDA"].source, "my-quote");
    assert_eq!(
        saved["forecast:oaxaca:2026-11-20"].value["summary"],
        "light rain"
    );
    assert_eq!(
        saved["forecast:oaxaca:2026-11-20"].value["precipitation"],
        0.4
    );
    assert!(
        saved["forecast:oaxaca:2026-12-05"].value["error"]
            .as_str()
            .unwrap()
            .starts_with("no forecast yet")
    );
    // The refreshed workspace evaluates with the new values.
    let check = std::process::Command::new(env!("CARGO_BIN_EXE_wtf"))
        .current_dir(&root)
        .args([
            "query",
            "--workspace",
            "diagnostics | where severity == \"error\"",
            "--fail-on-match",
        ])
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stdout)
    );
    let ws = Workspace::load(vec![root.clone()]).unwrap();
    let mut engine = Engine::at(&ws, now());
    assert_eq!(
        engine
            .named(&root.join("trip.wtf"), "usd")
            .unwrap()
            .display(),
        "$490.50"
    );
    assert_eq!(
        engine
            .named(&root.join("trip.wtf"), "nvda")
            .unwrap()
            .display(),
        "€42.50"
    );
}
