//! Cached lookups on disk, and fetching them: exchange rates, weather forecasts
//! and stock quotes, from built-in keyless providers or commands named in
//! `.xmd/providers.json`. Values live in `.xmd/lookups.json` with the time they
//! were fetched, so notes keep working offline. Fetching happens only on
//! `xmd refresh` or the Refresh lens.
use chrono::{DateTime, NaiveDate};
use eval::Workspace;
use eval::lookups::{Lookup, LookupKey, day_place};
use std::collections::BTreeMap;
use std::path::Path;

/// Every cached lookup, keyed by [`LookupKey`]'s text.
type Store = BTreeMap<String, Lookup>;

/// WMO weather interpretation codes, as Open-Meteo reports them.
fn weather_summary(code: i64) -> &'static str {
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
fn wanted(
    ws: &Workspace,
    now: DateTime<chrono::FixedOffset>,
    only: Option<&std::path::Path>,
) -> std::collections::BTreeSet<LookupKey> {
    let mut engine = eval::engine::Engine::at(ws, now);
    let mut keys = std::collections::BTreeSet::new();
    let today = engine.today();
    let symbols = ws.symbols();
    for (path, doc) in ws
        .documents
        .iter()
        .filter(|(path, _)| only.is_none_or(|only| *path == only))
    {
        for symbol in symbols.iter().filter(|s| s.path == *path) {
            let _ = engine.symbol(symbol);
        }
        for calculation in &doc.calculations {
            let _ = engine.eval_at(path, &calculation.source, calculation.span);
        }
        for task in &doc.tasks {
            for attr in task.attributes.values() {
                let _ = engine.eval(path, &attr.value);
            }
        }
        let dates = eval::itinerary::dates(&ws.modules, &doc.days, today);
        for (day, date) in doc.days.iter().zip(&dates) {
            if let (Some((places, _)), Some(date)) = (&day.places, date)
                && let Some(place) = day_place(places)
            {
                keys.insert(LookupKey::forecast(&place, *date));
            }
        }
    }
    keys.extend(engine.wanted().iter().cloned());
    keys
}

pub fn load(root: &Path) -> Store {
    std::fs::read(root.join(".xmd/lookups.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
pub fn save(root: &Path, store: &Store) -> Result<(), String> {
    let dir = root.join(".xmd");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(store).map_err(|e| e.to_string())?;
    let tmp = dir.join(format!("lookups-{}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    std::fs::rename(tmp, dir.join("lookups.json")).map_err(|e| e.to_string())
}
/// `.xmd/providers.json` maps a lookup kind to a command printing JSON,
/// with `{from}`, `{to}`, `{symbol}`, `{place}`, `{date}` placeholders.
fn providers(root: &Path) -> BTreeMap<String, String> {
    std::fs::read(root.join(".xmd/providers.json"))
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
            "Mozilla/5.0 xmd",
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
/// Fetch one key: (value, source). Unavailable data becomes a value with an
/// `error` field; request failures are returned to the refresh caller.
async fn fetch(
    root: &Path,
    key: &LookupKey,
    today: NaiveDate,
) -> Result<(serde_json::Value, String), String> {
    let providers = providers(root);
    match key {
        LookupKey::Forecast { place, date } => {
            fetch_forecast(&providers, place, *date, today).await
        }
        LookupKey::Rate { from, to } => {
            let (from, to) = (&from.to_string(), &to.to_string());
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
            let data: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
            let rate = data["rates"][to.as_str()]
                .as_f64()
                .ok_or_else(|| format!("No rate {from}→{to} from frankfurter.dev"))?;
            Ok((serde_json::json!({"rate": rate}), "frankfurter.dev".into()))
        }
        LookupKey::Quote(symbol) => {
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
            // command in .xmd/providers.json replaces it.
            let body = get(&format!(
                "https://query1.finance.yahoo.com/v8/finance/chart/{}?range=1d&interval=1d",
                encode(symbol)
            ))
            .await?;
            let data: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
            let meta = &data["chart"]["result"][0]["meta"];
            let price = meta["regularMarketPrice"].as_f64().ok_or_else(|| {
                format!("No quote for {symbol} from finance.yahoo.com; set a quote provider in .xmd/providers.json")
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
    }
}
/// Open-Meteo names the control run with the bare variable, and the other
/// runs with `_member01` through `_member50`. Ignore unrelated statistics.
fn ensemble_values<'a>(
    daily: &'a serde_json::Value,
    variable: &'a str,
    index: usize,
) -> impl Iterator<Item = f64> + 'a {
    daily
        .as_object()
        .into_iter()
        .flat_map(|fields| fields.iter())
        .filter_map(move |(name, values)| {
            let member = name == variable
                || name
                    .strip_prefix(variable)
                    .and_then(|suffix| suffix.strip_prefix("_member"))
                    .is_some_and(|member| {
                        member.len() == 2 && member.bytes().all(|c| c.is_ascii_digit())
                    });
            member
                .then(|| values[index].as_f64())
                .flatten()
                .filter(|value| value.is_finite())
        })
}
fn ensemble_mean(daily: &serde_json::Value, variable: &str, index: usize) -> Option<f64> {
    let (sum, count) = ensemble_values(daily, variable, index)
        .fold((0.0, 0), |(sum, count), value| (sum + value, count + 1));
    (count > 0).then(|| sum / f64::from(count))
}
fn seasonal_precipitation(daily: &serde_json::Value, index: usize) -> Option<f64> {
    // Estimate a wet day's probability from valid ensemble members, not
    // the ensemble's mean rainfall amount. Missing runs are not dry runs.
    let (wet, count) = ensemble_values(daily, "precipitation_sum", index)
        .filter(|amount| *amount >= 0.0)
        .fold((0, 0), |(wet, count), amount| {
            (wet + i32::from(amount > 0.1), count + 1)
        });
    // A lone run cannot supply an ensemble probability.
    (count > 1).then(|| f64::from(wet) / f64::from(count))
}
async fn fetch_forecast(
    providers: &BTreeMap<String, String>,
    place: &str,
    date: NaiveDate,
    today: NaiveDate,
) -> Result<(serde_json::Value, String), String> {
    if let Some(command) = providers.get("forecast") {
        let value = run(command, &[("place", place), ("date", &date.to_string())]).await?;
        return Ok((
            value,
            command
                .split_whitespace()
                .next()
                .unwrap_or("provider")
                .into(),
        ));
    }
    // Forecast days include today. Seasonal temperatures are ensemble means,
    // useful for planning but not a prediction of a specific day's weather.
    // https://open-meteo.com/en/docs/seasonal-forecast-api
    let days_ahead = (date - today).num_days();
    let seasonal = days_ahead >= 16;
    let source = if seasonal {
        "open-meteo.com (seasonal ensemble)"
    } else {
        "open-meteo.com"
    };
    let unavailable = || {
        (
            serde_json::json!({"error": if seasonal {
                "no forecast yet; seasonal outlooks cover about 7 months"
            } else {
                "no forecast yet for this date"
            }}),
            source.to_string(),
        )
    };
    if days_ahead >= 215 {
        return Ok(unavailable());
    }
    let geo = get(&format!(
        "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=en&format=json",
        encode(place)
    ))
    .await?;
    let geo: serde_json::Value = serde_json::from_str(&geo).map_err(|e| e.to_string())?;
    let hit = &geo["results"][0];
    let (Some(lat), Some(lon)) = (hit["latitude"].as_f64(), hit["longitude"].as_f64()) else {
        return Ok((
            serde_json::json!({"error": format!("Unknown place '{place}'")}),
            "open-meteo.com".into(),
        ));
    };
    let (endpoint, variables, model) = if seasonal {
        (
            "https://seasonal-api.open-meteo.com/v1/seasonal",
            "temperature_2m_max,temperature_2m_min,precipitation_sum",
            "&models=ecmwf_seasonal_seamless&precipitation_unit=mm",
        )
    } else {
        (
            "https://api.open-meteo.com/v1/forecast",
            "temperature_2m_max,temperature_2m_min,weather_code,precipitation_probability_max",
            "",
        )
    };
    let body = get(&format!(
        "{endpoint}?latitude={lat}&longitude={lon}&daily={variables}&timezone=auto&start_date={date}&end_date={date}{model}"
    ))
    .await?;
    let data: serde_json::Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
    let daily = &data["daily"];
    let date = date.to_string();
    let Some(index) = daily["time"]
        .as_array()
        .and_then(|times| times.iter().position(|time| time.as_str() == Some(&date)))
    else {
        return Ok(unavailable());
    };
    let temperature = |variable: &str| {
        if seasonal {
            ensemble_mean(daily, variable, index)
        } else {
            daily[variable][index].as_f64()
        }
    };
    let (Some(high), Some(low)) = (
        temperature("temperature_2m_max"),
        temperature("temperature_2m_min"),
    ) else {
        return Ok(unavailable());
    };
    let summary = if seasonal {
        "seasonal outlook (estimate)"
    } else {
        weather_summary(daily["weather_code"][index].as_i64().unwrap_or(-1))
    };
    let precipitation = if seasonal {
        seasonal_precipitation(daily, index)
    } else {
        daily["precipitation_probability_max"][index]
            .as_f64()
            .map(|p| p / 100.0)
    };
    Ok((
        serde_json::json!({
            "high": high, "low": low,
            "summary": summary,
            "precipitation": precipitation,
            "place": hit["name"].as_str().unwrap_or(place),
        }),
        source.into(),
    ))
}
/// Refresh every lookup the notes want, at the host's clock; returns the
/// errors.
pub async fn refresh(
    ws: &mut Workspace,
    now: DateTime<chrono::FixedOffset>,
    only: Option<&Path>,
) -> Vec<String> {
    let root = ws.root().to_path_buf();
    let keys = wanted(ws, now, only);
    let mut errors = Vec::new();
    for key in keys {
        match fetch(&root, &key, now.date_naive()).await {
            Ok((value, source)) => {
                ws.lookups.insert(
                    key.to_string(),
                    Lookup {
                        value,
                        fetched_at: now.to_utc(),
                        source,
                    },
                );
            }
            Err(e) => errors.push(format!("{}: {e}", key.describe())),
        }
    }
    if let Err(e) = save(&root, &ws.lookups) {
        errors.push(e);
    }
    errors
}
