//! External data behind explicit refreshes: exchange rates, weather forecasts
//! and stock quotes. Notes read cached values with the time they were fetched,
//! so they keep working offline and every badge can show its age. A host
//! (`lsp/host`) stores them and fetches them on an explicit refresh.
use crate::{
    engine_impl::{Currency, Value, decimal},
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
/// format of `.xmd/lookups.json`.
pub(crate) type Store = BTreeMap<String, Lookup>;

/// What a note asked the world for. `Display` writes the key `.xmd/lookups.json`
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

pub(crate) fn rate(store: &Store, from: Currency, to: Currency) -> EvalResult<f64> {
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
pub(crate) fn quote(store: &Store, symbol: &str) -> EvalResult<Value> {
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
pub(crate) fn forecast(
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
