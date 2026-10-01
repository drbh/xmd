//! Cached lookups on disk, and fetching them: whatever `cached(kind, key)`
//! asked for (exchange rates, weather forecasts and stock quotes, from the
//! prelude), from provider modules (the bundled ones live in `stdlib/providers`)
//! or commands named in `.xmd/providers.json`. Values live in
//! `.xmd/lookups.json` with the time they were fetched, so notes keep working
//! offline. Fetching happens only on `xmd refresh` or the Refresh lens.
use crate::command::Step;
use crate::io::{output, read_json_or_default, write_json_atomic};
use chrono::{DateTime, FixedOffset, NaiveDate};
use lang::eval::Workspace;
use lang::eval::engine::Value;
use lang::eval::lookups::{Lookup, LookupKey, Store};
use lang::eval::modules::{Module, ModuleKind, ModuleRegistry, from_json, json, record};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

/// Every lookup the notes read, found by evaluating them, and every one a
/// record a module built asks for (a day's forecast, say) without any call
/// that reads it.
fn wanted(
    ws: &Workspace,
    now: DateTime<FixedOffset>,
    only: Option<&Path>,
) -> std::collections::BTreeSet<LookupKey> {
    let mut engine = lang::eval::engine::Engine::at(ws, now);
    let records = services::Records::default();
    ws.documents()
        .keys()
        .filter(|path| only.is_none_or(|only| *path == only))
        .flat_map(|path| services::lookups(&records, &mut engine, path))
        .map(|(_, key)| key)
        .collect()
}

fn save(root: &Path, store: &Store) -> Result<(), String> {
    write_json_atomic(&root.join(".xmd/lookups.json"), store)
}
/// `.xmd/providers.json` maps a lookup kind to a command printing JSON, with
/// a placeholder for each part of the key: `{from}`, `{to}`, `{symbol}`,
/// `{place}`, `{date}`.
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
    child
        .args(["-sS", "--fail", "-L", "--max-time", "15"])
        .args(["-A", "Mozilla/5.0 xmd", url]);
    let stdout = output(&mut child, "Request timed out", "curl", "Request").await?;
    Ok(String::from_utf8_lossy(&stdout).into_owned())
}
/// Fetch one key: (value, source). A command in `.xmd/providers.json` answers
/// first; otherwise the first active provider module for this kind of lookup
/// does, so a workspace's own provider replaces a bundled one. Unavailable
/// data becomes a value with an `error` field; request failures are returned
/// to the refresh caller. What a value means is the prelude's, so it is kept
/// as the provider printed it.
async fn fetch(
    modules: &ModuleRegistry,
    root: &Path,
    key: &LookupKey,
    today: NaiveDate,
    now: DateTime<FixedOffset>,
) -> Result<(serde_json::Value, String), String> {
    let kind = key.kind();
    if let Some(command) = providers(root).get(kind) {
        let fills: Vec<(&str, String)> = key
            .parts()
            .iter()
            .map(|(name, value)| (name.as_str(), value.display()))
            .collect();
        let value = run(command, &fills).await?;
        let source = command.split_whitespace().next().unwrap_or("provider");
        return Ok((value, source.into()));
    }
    let module = modules
        .of_kind(ModuleKind::Provider)
        .find(|m| m.provides.iter().any(|provides| provides == kind))
        .ok_or_else(|| format!("No provider module answers {kind} lookups"))?;
    let key_record = [("kind".to_owned(), Value::Text(kind.to_owned()))]
        .into_iter()
        .chain(key.parts().iter().cloned());
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
            Some(Value::List(requests)) => Arc::unwrap_or_clone(requests).into_inner(),
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
    now: DateTime<FixedOffset>,
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
            Err(e) => errors.push(format!("{}: {e}", key.label())),
        }
    }
    if let Err(e) = save(&root, ws.lookups()) {
        errors.push(e);
    }
    errors
}
