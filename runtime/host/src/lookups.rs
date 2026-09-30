//! Cached lookups on disk, and fetching them: exchange rates, weather forecasts
//! and stock quotes, from provider modules (the bundled ones live in
//! `lang/plugins`) or commands named in `.xmd/providers.json`. Values live in `.xmd/lookups.json` with the time they
//! were fetched, so notes keep working offline. Fetching happens only on
//! `xmd refresh` or the Refresh lens.
use crate::command::Step;
use crate::io::{output, read_json_or_default, write_json_atomic};
use chrono::{DateTime, FixedOffset, NaiveDate};
use lang::eval::Workspace;
use lang::eval::engine::Value;
use lang::eval::lookups::{Lookup, LookupKey, Store, day_place};
use lang::eval::modules::{Module, ModuleKind, ModuleRegistry, from_json, json, record};
use lang::syntax::AttributeKey;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

/// Every lookup the notes ask for, found by evaluating them. Itinerary days
/// with a place want a forecast even without a `forecast(...)` call.
fn wanted(
    ws: &Workspace,
    now: DateTime<chrono::FixedOffset>,
    only: Option<&std::path::Path>,
) -> std::collections::BTreeSet<LookupKey> {
    let mut engine = lang::eval::engine::Engine::at(ws, now);
    let mut keys = std::collections::BTreeSet::new();
    let today = engine.today();
    let symbols = ws.symbols();
    for (path, doc) in ws
        .documents()
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
            // Only an expression can ask for a lookup.
            for (key, attr) in &task.attributes {
                if key
                    .parse::<AttributeKey>()
                    .is_ok_and(|k| k.value().is_expression())
                {
                    let _ = engine.eval(path, &attr.value);
                }
            }
        }
        // Like the evaluations above, a failure is the note's diagnostic to
        // report; days without dates want no forecast.
        let dates =
            lang::eval::itinerary::dates(ws.modules(), &doc.days, today).unwrap_or_default();
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

fn save(root: &Path, store: &Store) -> Result<(), String> {
    write_json_atomic(&root.join(".xmd/lookups.json"), store)
}
/// `.xmd/providers.json` maps a lookup kind to a command printing JSON,
/// with `{from}`, `{to}`, `{symbol}`, `{place}`, `{date}` placeholders.
fn providers(root: &Path) -> BTreeMap<String, String> {
    read_json_or_default(&root.join(".xmd/providers.json"))
}
async fn run(command: &str, fills: &[(&str, String)]) -> Result<serde_json::Value, String> {
    let mut text = command.to_string();
    for (name, value) in fills {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    let mut parts = text.split_whitespace();
    let program = parts.next().ok_or("Empty provider command")?;
    let mut child = tokio::process::Command::new(program);
    child.args(parts);
    let stdout = output(&mut child, "Provider timed out", program, program).await?;
    serde_json::from_slice(&stdout).map_err(|e| format!("{program} printed invalid JSON: {e}"))
}
async fn get(url: &str) -> Result<String, String> {
    let mut child = tokio::process::Command::new("curl");
    child.args([
        "-sS",
        "--fail",
        "-L",
        "--max-time",
        "15",
        "-A",
        "Mozilla/5.0 xmd",
        url,
    ]);
    let stdout = output(&mut child, "Request timed out", "curl", "Request").await?;
    Ok(String::from_utf8_lossy(&stdout).into_owned())
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
    let (kind, fields) = (key.kind(), key.fields());
    if let Some(command) = providers(root).get(kind.as_ref()) {
        let value = run(command, &fields).await?;
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
        .of_kind(ModuleKind::Provider)
        .find(|m| m.provides.contains(&kind))
        .ok_or_else(|| format!("No provider module answers {kind} lookups"))?;
    let key_record =
        [("kind", Value::Text(kind.to_string()))]
            .into_iter()
            .chain(fields.into_iter().map(|(name, value)| match (key, name) {
                (LookupKey::Forecast { date, .. }, "date") => (name, Value::Date(*date)),
                _ => (name, Value::Text(value)),
            }));
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

/// How many steps a provider may take for one lookup.
pub const PROVIDER_STEPS: usize = 16;

/// Run a provider module's `step` loop, performing only its HTTP requests.
async fn run_provider(
    module: &Module,
    key: Value,
    today: NaiveDate,
    now: DateTime<FixedOffset>,
) -> Result<(serde_json::Value, String), String> {
    let mut state = Value::Null;
    let mut results = Vec::new();
    for _ in 0..PROVIDER_STEPS {
        let input = record([
            ("key", key.clone()),
            ("today", Value::Date(today)),
            ("state", state),
            ("results", Value::list(results)),
        ]);
        let mut step = Step::call(module, input, now)?;
        step.failed()?;
        if step.done() {
            let mut value = json(&step.take("value").unwrap_or(Value::Null))?;
            decimals(&mut value);
            let source = step
                .take("source")
                .map(|source| source.display())
                .unwrap_or_else(|| module.id.clone());
            return Ok((value, source));
        }
        state = step.state();
        let requests = match step.take("requests") {
            Some(Value::List(requests)) => Arc::unwrap_or_clone(requests),
            _ => vec![],
        };
        results = Vec::new();
        for request in &requests {
            let request = json(request)?;
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
pub(crate) async fn refresh(
    ws: &mut Workspace,
    now: DateTime<chrono::FixedOffset>,
    only: Option<&Path>,
) -> Vec<String> {
    let root = ws.root().to_path_buf();
    let keys = wanted(ws, now, only);
    let modules = ws.modules().clone();
    let mut errors = Vec::new();
    for key in keys {
        match fetch(&modules, &root, &key, now.date_naive(), now).await {
            Ok((value, source)) => {
                ws.store_lookup(key.to_string(), Lookup::new(value, now.to_utc(), source));
            }
            Err(e) => errors.push(format!("{}: {e}", key.describe())),
        }
    }
    if let Err(e) = save(&root, ws.lookups()) {
        errors.push(e);
    }
    errors
}
