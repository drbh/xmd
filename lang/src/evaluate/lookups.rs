//! External data behind explicit refreshes: exchange rates, weather forecasts
//! and stock quotes. Values live in `.wtf/lookups.json` with the time they were
//! fetched, so notes keep working offline and every badge can show its age.
//! Fetching happens only in the native app, on `wtf refresh` or the Refresh
//! lens, through built-in keyless providers or commands from
//! `.wtf/providers.json`.
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
/// Keyed by the spelling `LookupKey` displays, because that is the on-disk
/// format of `.wtf/lookups.json`.
pub type Store = BTreeMap<String, Lookup>;

/// What a note asked the world for. `Display` writes the key `.wtf/lookups.json`
/// is stored under and `FromStr` reads one back, so the spelling is defined
/// once instead of being formatted and re-parsed at every use.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LookupKey {
    Rate { from: Currency, to: Currency },
    Quote(String),
    Forecast { place: String, date: NaiveDate },
}
impl LookupKey {
    pub fn rate(from: Currency, to: Currency) -> Self {
        Self::Rate { from, to }
    }
    /// Tickers are compared in upper case, so `quote(nvda)` and `quote(NVDA)`
    /// share one cache entry.
    pub fn quote(symbol: &str) -> Self {
        Self::Quote(symbol.to_ascii_uppercase())
    }
    /// Place names are compared trimmed and lowercased, for the same reason.
    pub fn forecast(place: &str, date: NaiveDate) -> Self {
        Self::Forecast {
            place: place.trim().to_lowercase(),
            date,
        }
    }
    /// A readable form for hovers: `rate EUR→USD`.
    pub fn describe(&self) -> String {
        match self {
            Self::Rate { from, to } => format!("rate {from}→{to}"),
            Self::Quote(symbol) => format!("quote {symbol}"),
            Self::Forecast { place, date } => format!("forecast {place} {date}"),
        }
    }
    pub fn lookup<'s>(&self, store: &'s Store) -> Option<&'s Lookup> {
        store.get(&self.to_string())
    }
}
impl std::fmt::Display for LookupKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rate { from, to } => write!(f, "rate:{from}:{to}"),
            Self::Quote(symbol) => write!(f, "quote:{symbol}"),
            Self::Forecast { place, date } => write!(f, "forecast:{place}:{date}"),
        }
    }
}
impl std::str::FromStr for LookupKey {
    type Err = String;
    fn from_str(key: &str) -> Result<Self, String> {
        // A place may contain colons, so a forecast key is read from the right.
        if let Some(rest) = key.strip_prefix("forecast:") {
            let (place, date) = rest.rsplit_once(':').ok_or("Malformed forecast key")?;
            let date = date.parse().map_err(|_| "Malformed forecast date")?;
            return Ok(Self::forecast(place, date));
        }
        let currency =
            |code: &str| Currency::parse(code).ok_or_else(|| format!("Unknown currency '{code}'"));
        match key.splitn(3, ':').collect::<Vec<_>>().as_slice() {
            ["rate", from, to] => Ok(Self::rate(currency(from)?, currency(to)?)),
            ["quote", symbol] => Ok(Self::quote(symbol)),
            _ => Err(format!("Unknown lookup '{key}'")),
        }
    }
}
/// Ordered by the spelling, so a sorted list of keys reads the same way the
/// store and the file do.
impl Ord for LookupKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.to_string().cmp(&other.to_string())
    }
}
impl PartialOrd for LookupKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
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

pub fn rate(store: &Store, from: Currency, to: Currency) -> Result<f64, String> {
    if from == to {
        return Ok(1.0);
    }
    let lookup = LookupKey::rate(from, to).lookup(store).ok_or_else(|| {
        format!("No cached rate {from}→{to}; run wtf refresh or use the ⟳ lookups lens")
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
    let lookup = LookupKey::quote(symbol).lookup(store).ok_or_else(|| {
        format!("No cached quote for {symbol}; run wtf refresh or use the ⟳ lookups lens")
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
    let lookup = LookupKey::forecast(place, date).lookup(store).ok_or_else(|| {
        format!(
            "No cached forecast for {place} on {date}; run wtf refresh or use the ⟳ lookups lens"
        )
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
    now: DateTime<chrono::FixedOffset>,
) -> std::collections::BTreeSet<LookupKey> {
    let mut engine = crate::engine::Engine::at(ws, now);
    let today = engine.today;
    for (path, doc) in &ws.documents {
        for symbol in ws.symbols().into_iter().filter(|s| s.path == *path) {
            let _ = engine.symbol(&symbol);
        }
        for task in &doc.tasks {
            for attr in task.attributes.values() {
                let _ = engine.eval(path, &attr.value);
            }
        }
        let dates = crate::itinerary::dates(&ws.modules, &doc.days, today);
        for (day, date) in doc.days.iter().zip(&dates) {
            if let (Some((places, _)), Some(date)) = (&day.places, date)
                && let Some(place) = day_place(places)
            {
                engine.wanted.push(LookupKey::forecast(&place, *date));
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
        std::fs::read(root.join(".wtf/lookups.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    pub fn save(root: &Path, store: &Store) -> Result<(), String> {
        let dir = root.join(".wtf");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec_pretty(store).map_err(|e| e.to_string())?;
        let tmp = dir.join(format!("lookups-{}.tmp", std::process::id()));
        std::fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
        std::fs::rename(tmp, dir.join("lookups.json")).map_err(|e| e.to_string())
    }
    /// `.wtf/providers.json` maps a lookup kind to a command printing JSON,
    /// with `{from}`, `{to}`, `{symbol}`, `{place}`, `{date}` placeholders.
    fn providers(root: &Path) -> BTreeMap<String, String> {
        std::fs::read(root.join(".wtf/providers.json"))
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
                "Mozilla/5.0 wtf",
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
    pub async fn fetch(
        root: &Path,
        key: &LookupKey,
    ) -> Result<(serde_json::Value, String), String> {
        let providers = providers(root);
        match key {
            LookupKey::Forecast { place, date } => {
                fetch_forecast(&providers, place, &date.to_string()).await
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
                let data: serde_json::Value =
                    serde_json::from_str(&body).map_err(|e| e.to_string())?;
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
                // command in .wtf/providers.json replaces it.
                let body = get(&format!(
                    "https://query1.finance.yahoo.com/v8/finance/chart/{}?range=1d&interval=1d",
                    encode(symbol)
                ))
                .await?;
                let data: serde_json::Value =
                    serde_json::from_str(&body).map_err(|e| e.to_string())?;
                let meta = &data["chart"]["result"][0]["meta"];
                let price = meta["regularMarketPrice"].as_f64().ok_or_else(|| {
                    format!("No quote for {symbol} from finance.yahoo.com; set a quote provider in .wtf/providers.json")
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
    /// Refresh every lookup the notes want, at the host's clock; returns the
    /// errors.
    pub async fn refresh(
        ws: &mut crate::workspace::Workspace,
        now: DateTime<chrono::FixedOffset>,
    ) -> Vec<String> {
        let root = ws.root().to_path_buf();
        let keys = super::wanted(ws, now);
        let mut errors = Vec::new();
        for key in keys {
            match fetch(&root, &key).await {
                Ok((value, source)) => {
                    ws.lookups.insert(
                        key.to_string(),
                        Lookup {
                            value,
                            fetched_at: Utc::now(),
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
}
