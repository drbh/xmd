//! External data behind explicit refreshes: exchange rates, weather forecasts
//! and stock quotes. Values live in `.jot/lookups.json` with the time they were
//! fetched, so notes keep working offline and every badge can show its age.
//! Fetching happens only in the native app, on `jot refresh` or the Refresh
//! lens, through built-in keyless providers or commands from
//! `.jot/providers.json`.
use crate::engine::{Currency, Forecast, Value};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Lookup {
    pub value: serde_json::Value,
    pub fetched_at: DateTime<Utc>,
    pub source: String,
}
pub type Store = BTreeMap<String, Lookup>;

pub fn rate_key(from: Currency, to: Currency) -> String {
    format!("rate:{from}:{to}")
}
pub fn quote_key(symbol: &str) -> String {
    format!("quote:{}", symbol.to_ascii_uppercase())
}
pub fn forecast_key(place: &str, date: NaiveDate) -> String {
    format!("forecast:{}:{date}", place.trim().to_lowercase())
}
/// The place a day's forecast is for: the destination when the heading lists
/// a route such as `New York | Oaxaca`.
pub fn day_place(places: &str) -> Option<String> {
    places
        .rsplit(['|', '→', '>'])
        .map(str::trim)
        .find(|p| !p.is_empty())
        .map(str::to_string)
}
/// A readable form of a key for hovers: `rate EUR→USD`.
pub fn describe(key: &str) -> String {
    if let Some(rest) = key.strip_prefix("forecast:") {
        let (place, date) = rest.rsplit_once(':').unwrap_or((rest, ""));
        return format!("forecast {place} {date}");
    }
    let parts: Vec<&str> = key.splitn(3, ':').collect();
    match parts.as_slice() {
        ["rate", from, to] => format!("rate {from}→{to}"),
        ["quote", symbol] => format!("quote {symbol}"),
        _ => key.to_string(),
    }
}

pub fn rate(store: &Store, from: Currency, to: Currency) -> Result<f64, String> {
    if from == to {
        return Ok(1.0);
    }
    let key = rate_key(from, to);
    let lookup = store.get(&key).ok_or_else(|| {
        format!("No cached rate {from}→{to}; run jot refresh or use Refresh lookups")
    })?;
    lookup.value["rate"]
        .as_f64()
        .filter(|r| r.is_finite() && *r > 0.0)
        .ok_or_else(|| {
            lookup.value["error"]
                .as_str()
                .map(|e| format!("Rate {from}→{to}: {e}"))
                .unwrap_or_else(|| format!("Cached rate {from}→{to} is unreadable"))
        })
}
pub fn quote(store: &Store, symbol: &str) -> Result<Value, String> {
    let key = quote_key(symbol);
    let lookup = store.get(&key).ok_or_else(|| {
        format!("No cached quote for {symbol}; run jot refresh or use Refresh lookups")
    })?;
    let price = lookup.value["price"]
        .as_f64()
        .filter(|p| p.is_finite())
        .ok_or_else(|| {
            lookup.value["error"]
                .as_str()
                .map(|e| format!("Quote {symbol}: {e}"))
                .unwrap_or_else(|| format!("Cached quote for {symbol} is unreadable"))
        })?;
    let currency = lookup.value["currency"]
        .as_str()
        .and_then(Currency::parse)
        .unwrap_or(Currency::USD);
    Ok(Value::Money(price, currency))
}
pub fn forecast(
    store: &Store,
    place: &str,
    date: NaiveDate,
    fahrenheit: bool,
) -> Result<Forecast, String> {
    let key = forecast_key(place, date);
    let lookup = store.get(&key).ok_or_else(|| {
        format!("No cached forecast for {place} on {date}; run jot refresh or use Refresh lookups")
    })?;
    forecast_from(&lookup.value, fahrenheit)
        .map_err(|e| format!("Forecast for {place} on {date}: {e}"))
}
pub fn forecast_from(value: &serde_json::Value, fahrenheit: bool) -> Result<Forecast, String> {
    if let Some(error) = value["error"].as_str() {
        return Err(error.to_string());
    }
    let (high, low) = (
        value["high"].as_f64().ok_or("missing high temperature")?,
        value["low"].as_f64().ok_or("missing low temperature")?,
    );
    let convert = |c: f64| {
        if fahrenheit {
            (c * 9.0 / 5.0 + 32.0).round()
        } else {
            c.round()
        }
    };
    Ok(Forecast {
        high: convert(high),
        low: convert(low),
        summary: value["summary"].as_str().unwrap_or("").to_string(),
        precipitation: value["precipitation"].as_f64(),
        fahrenheit,
    })
}
/// WMO weather interpretation codes, as Open-Meteo reports them.
pub fn weather_summary(code: i64) -> &'static str {
    match code {
        0 => "clear",
        1 => "mostly clear",
        2 => "partly cloudy",
        3 => "overcast",
        45 | 48 => "fog",
        51 | 53 | 55 => "drizzle",
        56 | 57 => "freezing drizzle",
        61 => "light rain",
        63 => "rain",
        65 => "heavy rain",
        66 | 67 => "freezing rain",
        71 => "light snow",
        73 => "snow",
        75 => "heavy snow",
        77 => "snow grains",
        80 | 81 => "showers",
        82 => "heavy showers",
        85 | 86 => "snow showers",
        95 => "thunderstorm",
        96 | 99 => "thunderstorm with hail",
        _ => "unknown",
    }
}

/// Every lookup the notes ask for, found by evaluating them. Itinerary days
/// with a place want a forecast even without a `forecast(...)` call.
pub fn wanted(
    ws: &crate::workspace::Workspace,
    today: NaiveDate,
) -> std::collections::BTreeSet<String> {
    let mut engine = crate::engine::Engine::new(ws, today);
    for (path, doc) in &ws.documents {
        for symbol in ws.symbols().into_iter().filter(|s| s.path == *path) {
            let _ = engine.symbol(&symbol);
        }
        for task in &doc.tasks {
            for attr in task.attributes.values() {
                let _ = engine.eval(path, &attr.value);
            }
        }
        let dates = crate::itinerary::dates(&doc.days, today);
        for (day, date) in doc.days.iter().zip(&dates) {
            if let (Some((places, _)), Some(date)) = (&day.places, date)
                && let Some(place) = day_place(places)
            {
                engine.wanted.push(forecast_key(&place, *date));
            }
        }
    }
    engine.wanted.into_iter().collect()
}

#[cfg(feature = "native")]
pub mod native {
    use super::*;
    use std::path::Path;

    pub fn load(root: &Path) -> Store {
        std::fs::read(root.join(".jot/lookups.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    pub fn save(root: &Path, store: &Store) -> Result<(), String> {
        let dir = root.join(".jot");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec_pretty(store).map_err(|e| e.to_string())?;
        let tmp = dir.join(format!("lookups-{}.tmp", std::process::id()));
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(tmp, dir.join("lookups.json")).map_err(|e| e.to_string())
    }
    /// `.jot/providers.json` maps a lookup kind to a command printing JSON,
    /// with `{from}`, `{to}`, `{symbol}`, `{place}`, `{date}` placeholders.
    fn providers(root: &Path) -> BTreeMap<String, String> {
        std::fs::read(root.join(".jot/providers.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    async fn run(command: &str, fills: &[(&str, &str)]) -> Result<serde_json::Value, String> {
        let mut text = command.to_string();
        for (name, value) in fills {
            text = text.replace(&format!("{{{name}}}"), value);
        }
        let mut parts = text.split_whitespace();
        let program = parts.next().ok_or("Empty provider command")?;
        let mut child = tokio::process::Command::new(program);
        child.args(parts).kill_on_drop(true);
        let output = tokio::time::timeout(std::time::Duration::from_secs(20), child.output())
            .await
            .map_err(|_| "Provider timed out".to_string())?
            .map_err(|e| format!("Cannot run {program}: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "{program} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        serde_json::from_slice(&output.stdout)
            .map_err(|e| format!("{program} printed invalid JSON: {e}"))
    }
    async fn get(url: &str) -> Result<String, String> {
        let mut child = tokio::process::Command::new("curl");
        child
            .args([
                "-sS",
                "--fail",
                "-L",
                "--max-time",
                "15",
                "-A",
                "Mozilla/5.0 jot",
                url,
            ])
            .kill_on_drop(true);
        let output = tokio::time::timeout(std::time::Duration::from_secs(20), child.output())
            .await
            .map_err(|_| "Request timed out".to_string())?
            .map_err(|e| format!("Cannot run curl: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "Request failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
    fn encode(s: &str) -> String {
        url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
    }
    /// Fetch one key: (value, source). Provider errors become a value with an
    /// `error` field, so the note shows the problem instead of a stale success.
    pub async fn fetch(root: &Path, key: &str) -> Result<(serde_json::Value, String), String> {
        let providers = providers(root);
        if let Some(rest) = key.strip_prefix("forecast:") {
            let (place, date) = rest.rsplit_once(':').ok_or("Malformed forecast key")?;
            return fetch_forecast(&providers, place, date).await;
        }
        let parts: Vec<&str> = key.splitn(3, ':').collect();
        match parts.as_slice() {
            ["rate", from, to] => {
                if let Some(command) = providers.get("rate") {
                    let value = run(command, &[("from", from), ("to", to)]).await?;
                    return Ok((
                        serde_json::json!({"rate": value["rate"]}),
                        command
                            .split_whitespace()
                            .next()
                            .unwrap_or("provider")
                            .into(),
                    ));
                }
                let body = get(&format!(
                    "https://api.frankfurter.dev/v1/latest?base={from}&symbols={to}"
                ))
                .await?;
                let data: serde_json::Value =
                    serde_json::from_str(&body).map_err(|e| e.to_string())?;
                let rate = data["rates"][*to]
                    .as_f64()
                    .ok_or_else(|| format!("No rate {from}→{to} from frankfurter.dev"))?;
                Ok((serde_json::json!({"rate": rate}), "frankfurter.dev".into()))
            }
            ["quote", symbol] => {
                if let Some(command) = providers.get("quote") {
                    let value = run(command, &[("symbol", symbol)]).await?;
                    return Ok((
                        serde_json::json!({"price": value["price"], "currency": value["currency"].as_str().unwrap_or("USD")}),
                        command
                            .split_whitespace()
                            .next()
                            .unwrap_or("provider")
                            .into(),
                    ));
                }
                // Yahoo's chart endpoint is unofficial but keyless; a provider
                // command in .jot/providers.json replaces it.
                let body = get(&format!(
                    "https://query1.finance.yahoo.com/v8/finance/chart/{}?range=1d&interval=1d",
                    encode(symbol)
                ))
                .await?;
                let data: serde_json::Value =
                    serde_json::from_str(&body).map_err(|e| e.to_string())?;
                let meta = &data["chart"]["result"][0]["meta"];
                let price = meta["regularMarketPrice"].as_f64().ok_or_else(|| {
                    format!("No quote for {symbol} from finance.yahoo.com; set a quote provider in .jot/providers.json")
                })?;
                Ok((
                    serde_json::json!({
                        "price": price,
                        "currency": meta["currency"].as_str().unwrap_or("USD"),
                        "change_percent": meta["regularMarketChangePercent"],
                    }),
                    "finance.yahoo.com".into(),
                ))
            }
            _ => Err(format!("Unknown lookup '{key}'")),
        }
    }
    async fn fetch_forecast(
        providers: &BTreeMap<String, String>,
        place: &str,
        date: &str,
    ) -> Result<(serde_json::Value, String), String> {
        {
            {
                if let Some(command) = providers.get("forecast") {
                    let value = run(command, &[("place", place), ("date", date)]).await?;
                    return Ok((
                        value,
                        command
                            .split_whitespace()
                            .next()
                            .unwrap_or("provider")
                            .into(),
                    ));
                }
                let geo = get(&format!(
                    "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=en&format=json",
                    encode(place)
                ))
                .await?;
                let geo: serde_json::Value =
                    serde_json::from_str(&geo).map_err(|e| e.to_string())?;
                let hit = &geo["results"][0];
                let (Some(lat), Some(lon)) = (hit["latitude"].as_f64(), hit["longitude"].as_f64())
                else {
                    return Ok((
                        serde_json::json!({"error": format!("Unknown place '{place}'")}),
                        "open-meteo.com".into(),
                    ));
                };
                let body = get(&format!(
                    "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&daily=temperature_2m_max,temperature_2m_min,weather_code,precipitation_probability_max&timezone=auto&start_date={date}&end_date={date}"
                ))
                .await;
                let body = match body {
                    Ok(body) => body,
                    Err(_) => {
                        return Ok((
                            serde_json::json!({"error": "no forecast yet; forecasts cover about 16 days"}),
                            "open-meteo.com".into(),
                        ));
                    }
                };
                let data: serde_json::Value =
                    serde_json::from_str(&body).map_err(|e| e.to_string())?;
                let daily = &data["daily"];
                let (Some(high), Some(low)) = (
                    daily["temperature_2m_max"][0].as_f64(),
                    daily["temperature_2m_min"][0].as_f64(),
                ) else {
                    return Ok((
                        serde_json::json!({"error": "no forecast yet; forecasts cover about 16 days"}),
                        "open-meteo.com".into(),
                    ));
                };
                let code = daily["weather_code"][0].as_i64().unwrap_or(-1);
                let precipitation = daily["precipitation_probability_max"][0]
                    .as_f64()
                    .map(|p| p / 100.0);
                Ok((
                    serde_json::json!({
                        "high": high, "low": low,
                        "summary": weather_summary(code),
                        "precipitation": precipitation,
                        "place": hit["name"].as_str().unwrap_or(place),
                    }),
                    "open-meteo.com".into(),
                ))
            }
        }
    }
    /// Refresh every lookup the notes want; returns the errors.
    pub async fn refresh(ws: &mut crate::workspace::Workspace) -> Vec<String> {
        let root = ws.root().to_path_buf();
        let keys = super::wanted(ws, chrono::Local::now().date_naive());
        let mut errors = Vec::new();
        for key in keys {
            match fetch(&root, &key).await {
                Ok((value, source)) => {
                    ws.lookups.insert(
                        key,
                        Lookup {
                            value,
                            fetched_at: Utc::now(),
                            source,
                        },
                    );
                }
                Err(e) => errors.push(format!("{}: {e}", describe(&key))),
            }
        }
        if let Err(e) = save(&root, &ws.lookups) {
            errors.push(e);
        }
        errors
    }
}
