//! Cached lookups on disk, and fetching them: exchange rates, weather forecasts
//! and stock quotes, from provider modules (the bundled ones live in
//! `lang/plugins`) or commands named in `.xmd/providers.json`. Values live in `.xmd/lookups.json` with the time they
//! were fetched, so notes keep working offline. Fetching happens only on
//! `xmd refresh` or the Refresh lens.
use chrono::{DateTime, FixedOffset, NaiveDate};
use eval::Workspace;
use eval::engine::Value;
use eval::lookups::{Lookup, LookupKey, day_place};
use eval::modules::{Hook, Module, ModuleKind, ModuleRegistry, from_json, json, record};
use std::collections::BTreeMap;
use std::path::Path;

/// Every cached lookup, keyed by [`LookupKey`]'s text.
type Store = BTreeMap<String, Lookup>;

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
/// Fetch one key: (value, source). A command in `.xmd/providers.json` answers
/// first; otherwise the first active provider module for this kind of lookup
/// does, so a workspace's own provider replaces a bundled one. Unavailable
/// data becomes a value with an `error` field; request failures are returned
/// to the refresh caller.
async fn fetch(
    modules: &ModuleRegistry,
    root: &Path,
    key: &LookupKey,
    today: NaiveDate,
    now: DateTime<FixedOffset>,
) -> Result<(serde_json::Value, String), String> {
    let (kind, fields) = match key {
        LookupKey::Rate { from, to } => (
            "rate",
            vec![("from", from.to_string()), ("to", to.to_string())],
        ),
        LookupKey::Quote(symbol) => ("quote", vec![("symbol", symbol.clone())]),
        LookupKey::Forecast { place, date } => (
            "forecast",
            vec![("place", place.clone()), ("date", date.to_string())],
        ),
    };
    if let Some(command) = providers(root).get(kind) {
        let fills: Vec<(&str, &str)> = fields.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let value = run(command, &fills).await?;
        let value = match key {
            LookupKey::Rate { .. } => serde_json::json!({"rate": value["rate"]}),
            LookupKey::Quote(_) => {
                serde_json::json!({"price": value["price"], "currency": value["currency"].as_str().unwrap_or("USD")})
            }
            LookupKey::Forecast { .. } => value,
        };
        let source = command.split_whitespace().next().unwrap_or("provider");
        return Ok((value, source.into()));
    }
    let module = modules
        .active()
        .find(|m| m.kind == ModuleKind::Provider && m.provides.iter().any(|p| p == kind))
        .ok_or_else(|| format!("No provider module answers {kind} lookups"))?;
    let mut key_record: Vec<(String, Value)> = vec![("kind".into(), Value::Text(kind.into()))];
    for (name, value) in fields {
        key_record.push(match (key, name) {
            (LookupKey::Forecast { date, .. }, "date") => (name.into(), Value::Date(*date)),
            _ => (name.into(), Value::Text(value)),
        });
    }
    run_provider(module, record(key_record), today, now).await
}

/// Store every number as a decimal, as measured and fetched values always
/// have been, so a whole reading such as 14 degrees reads back as 14.0.
fn decimals(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Number(n) => {
            if let Some(decimal) = n.as_f64().and_then(serde_json::Number::from_f64) {
                *n = decimal;
            }
        }
        serde_json::Value::Array(items) => items.iter_mut().for_each(decimals),
        serde_json::Value::Object(fields) => fields.values_mut().for_each(decimals),
        _ => {}
    }
}

/// Run a provider module's `step` loop, performing only its HTTP requests.
async fn run_provider(
    module: &Module,
    key: Value,
    today: NaiveDate,
    now: DateTime<FixedOffset>,
) -> Result<(serde_json::Value, String), String> {
    let mut state = Value::Null;
    let mut results = Vec::new();
    for _ in 0..16 {
        let input = record([
            ("key".into(), key.clone()),
            ("today".into(), Value::Date(today)),
            ("state".into(), state),
            ("results".into(), Value::List(results)),
        ]);
        let Value::Record(mut output) = module
            .call(Hook::Step, vec![input], now)
            .map_err(|e| e.to_string())?
        else {
            return Err(format!("{}: step must return a record", module.id));
        };
        match output.remove("error") {
            None | Some(Value::Null) => {}
            Some(error) => return Err(error.display()),
        }
        if matches!(output.get("done"), Some(Value::Bool(true))) {
            let mut value =
                json(&output.remove("value").unwrap_or(Value::Null)).map_err(|e| e.to_string())?;
            decimals(&mut value);
            let source = output
                .remove("source")
                .map(|source| source.display())
                .unwrap_or_else(|| module.id.clone());
            return Ok((value, source));
        }
        state = output.remove("state").unwrap_or(Value::Null);
        let requests = match output.remove("requests") {
            Some(Value::List(requests)) => requests,
            _ => vec![],
        };
        results = Vec::new();
        for request in &requests {
            let request = json(request).map_err(|e| e.to_string())?;
            if request["kind"].as_str() != Some("http") {
                return Err(format!(
                    "{}: a provider can only make http requests",
                    module.id
                ));
            }
            let url = request["url"]
                .as_str()
                .ok_or("An http request needs a url")?;
            // Every provider reads JSON, so a reply that is not JSON is a failed request.
            let reply = get(url).await.and_then(|body| {
                serde_json::from_str::<serde_json::Value>(&body).map_err(|e| e.to_string())
            });
            let answer = match reply {
                Ok(json) => serde_json::json!({"ok": true, "json": json}),
                Err(error) => serde_json::json!({"ok": false, "error": error}),
            };
            results.push(from_json(&answer));
        }
    }
    Err(format!("{} did not finish", module.id))
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
    let modules = ws.modules.clone();
    let mut errors = Vec::new();
    for key in keys {
        match fetch(&modules, &root, &key, now.date_naive(), now).await {
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
