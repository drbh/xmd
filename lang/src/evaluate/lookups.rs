//! External data behind explicit refreshes: exchange rates, weather forecasts
//! and stock quotes. Values live in `.wtf/lookups.json` with the time they were
//! fetched, so notes keep working offline and every badge can show its age.
//! Fetching happens only in the native app, on `wtf refresh` or the Refresh
//! lens, through built-in keyless providers or commands from
//! `.wtf/providers.json`.
use crate::{
    engine::{Currency, Value, decimal},
    error::{EvalError, EvalResult, PropertyOwner},
};

/// A day's weather from a cached lookup: an object the store owns and a note
/// reads from, not a kind the language can build.
#[derive(Clone, Debug, PartialEq)]
pub struct Forecast {
    pub high: f64,
    pub low: f64,
    pub summary: String,
    /// Chance of precipitation, 0 to 1, reported or estimated from an ensemble.
    pub precipitation: Option<f64>,
    pub fahrenheit: bool,
}
impl Forecast {
    pub fn display(&self) -> String {
        let unit = if self.fahrenheit { "°F" } else { "°C" };
        let mut s = format!(
            "{}{unit} / {}{unit} · {}",
            decimal(self.high),
            decimal(self.low),
            self.summary
        );
        if let Some(p) = self.precipitation
            && p >= 0.2
        {
            s.push_str(&format!(" · {}% rain", (p * 100.0).round()));
        }
        s
    }
    pub fn property(&self, name: &str) -> EvalResult<Value> {
        match name {
            "high" => Ok(Value::Number(self.high)),
            "low" => Ok(Value::Number(self.low)),
            "summary" => Ok(Value::Text(self.summary.clone())),
            "rain" => self
                .precipitation
                .map(Value::Ratio)
                .ok_or(EvalError::Message(
                    "This forecast has no precipitation chance".into(),
                )),
            _ => Err(EvalError::UnknownProperty {
                owner: PropertyOwner::Forecast,
                name: name.into(),
            }),
        }
    }
}
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

pub fn rate(store: &Store, from: Currency, to: Currency) -> EvalResult<f64> {
    if from == to {
        return Ok(1.0);
    }
    let key = LookupKey::rate(from, to);
    let lookup = key
        .lookup(store)
        .ok_or_else(|| EvalError::NotCached(key.clone()))?;
    lookup.value["rate"]
        .as_f64()
        .filter(|r| r.is_finite() && *r > 0.0)
        .ok_or_else(|| reported(lookup, key))
}
pub fn quote(store: &Store, symbol: &str) -> EvalResult<Value> {
    // The cache is keyed case-insensitively; a message names the ticker as written.
    let key = LookupKey::Quote(symbol.to_string());
    let lookup = LookupKey::quote(symbol)
        .lookup(store)
        .ok_or_else(|| EvalError::NotCached(key.clone()))?;
    let price = lookup.value["price"]
        .as_f64()
        .filter(|p| p.is_finite())
        .ok_or_else(|| reported(lookup, key))?;
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
) -> EvalResult<Forecast> {
    let key = LookupKey::Forecast {
        place: place.to_string(),
        date,
    };
    let lookup = LookupKey::forecast(place, date)
        .lookup(store)
        .ok_or_else(|| EvalError::NotCached(key.clone()))?;
    forecast_from(&lookup.value, fahrenheit).map_err(|e| e.in_lookup(key))
}
/// What a cached lookup said went wrong, or that it cannot be read at all.
fn reported(lookup: &Lookup, key: LookupKey) -> EvalError {
    match lookup.value["error"].as_str() {
        Some(message) => EvalError::Custom(message.into()).in_lookup(key),
        None => EvalError::Unreadable(key),
    }
}
pub fn forecast_from(value: &serde_json::Value, fahrenheit: bool) -> EvalResult<Forecast> {
    if let Some(error) = value["error"].as_str() {
        return Err(EvalError::Custom(error.to_string()));
    }
    let (high, low) = (
        value["high"]
            .as_f64()
            .ok_or(EvalError::Message("missing high temperature".into()))?,
        value["low"]
            .as_f64()
            .ok_or(EvalError::Message("missing low temperature".into()))?,
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
    only: Option<&std::path::Path>,
) -> std::collections::BTreeSet<LookupKey> {
    let mut engine = crate::engine::Engine::at(ws, now);
    let today = engine.today;
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
    /// Fetch one key: (value, source). Unavailable data becomes a value with an
    /// `error` field; request failures are returned to the refresh caller.
    pub async fn fetch(
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
        ws: &mut crate::workspace::Workspace,
        now: DateTime<chrono::FixedOffset>,
        only: Option<&Path>,
    ) -> Vec<String> {
        let root = ws.root().to_path_buf();
        let keys = super::wanted(ws, now, only);
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
}
