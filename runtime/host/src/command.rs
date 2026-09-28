//! Running a command module: `xmd run`. A command's `step(ctx)` hook is pure;
//! it returns the effects it wants and reads their results on the next step,
//! so everything a command does to the world passes through here, only when a
//! person runs it.
//!
//! Each step returns `{state, requests, report, done, error?, repeat_after?}`.
//! The runner prints `report` lines, stops on `error`, and otherwise performs
//! `requests` in order and calls `step` again with
//! `{args, dir, state, results}`, where `results[i]` answers `requests[i]`.
//! When `done` is true it finishes, or with `repeat_after: seconds` it waits
//! and starts over with fresh state (the command keeps anything durable in
//! its own files).
//!
//! Requests, each a record with a `kind`:
//! - `http {method, url, headers?, json?, pick?}` → `{ok, status, json, text}`;
//!   `pick` keeps only the named fields of each record in a JSON list reply,
//!   so a listing stays within the size a module call accepts.
//! - `read {path, json?}` → `{ok, text}`, or `{ok, json}` parsed when `json`
//!   is true; `list {path}` → `{ok, files}` (file names); `write {path, text}`
//!   or `write {path, json}` (saved as pretty JSON), `move {from, to}` and
//!   `remove {path}` → `{ok}`.
//!   Paths are relative to the directory the command runs in and cannot leave it.
//! - `credential {scope, set?}` → `{ok, value}`: a secret saved per command and
//!   scope in the user's config directory, stored when `set` is given.
//! - `env {name}` → `{ok, value}`, for `XMD_*` variables only.
//! - `uuid {}` → `{ok, value}`.
//!
//! A failed effect answers `{ok: false, error}` rather than stopping the run,
//! so the command decides what a failure means.
use chrono::{DateTime, FixedOffset};
use lang::eval::engine::Value;
use lang::eval::modules::{Hook, Module, ModuleKind, from_json, json};
use std::{
    collections::BTreeMap,
    io::Write as _,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
};

/// How many steps one run may take before the runner assumes a loop.
const MAX_STEPS: usize = 10_000;
/// How many effects one step may request.
const MAX_REQUESTS: usize = 256;

/// Run `module` in `dir` with the words after its name, printing its report
/// lines through `report`. `now` is the clock each run starts at.
pub fn run_command(
    module: &Module,
    dir: &Path,
    args: &[String],
    now: impl Fn() -> DateTime<FixedOffset>,
    mut report: impl FnMut(&str),
) -> Result<(), String> {
    if module.kind != ModuleKind::Command {
        return Err(format!(
            "{} is a {} module, not a command",
            module.id, module.kind
        ));
    }
    let dir = dir
        .canonicalize()
        .map_err(|e| format!("Cannot open {}: {e}", dir.display()))?;
    let context = Context {
        module: &module.id,
        dir: &dir,
    };
    let args = parse_args(args);
    loop {
        let clock = now();
        let mut state = Value::Null;
        let mut results = Vec::new();
        let mut finished = None;
        for _ in 0..MAX_STEPS {
            let input = lang::eval::modules::record([
                ("args".into(), args.clone()),
                (
                    "dir".into(),
                    Value::Text(
                        dir.file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                    ),
                ),
                ("state".into(), state),
                ("results".into(), Value::List(results)),
            ]);
            let output = module
                .call(Hook::Step, vec![input], clock)
                .map_err(|e| e.to_string())?;
            let Value::Record(mut fields) = output else {
                return Err(format!("{}: step must return a record", module.id));
            };
            if let Some(Value::List(lines)) = fields.get("report") {
                for line in lines {
                    report(&line.display());
                }
            }
            match fields.remove("error") {
                None | Some(Value::Null) => {}
                Some(error) => return Err(error.display()),
            }
            state = fields.remove("state").unwrap_or(Value::Null);
            let requests = match fields.remove("requests") {
                None | Some(Value::Null) => vec![],
                Some(Value::List(requests)) => requests,
                Some(_) => return Err(format!("{}: requests must be a list", module.id)),
            };
            if requests.len() > MAX_REQUESTS {
                return Err(format!(
                    "{}: a step may request at most {MAX_REQUESTS} effects",
                    module.id
                ));
            }
            results = requests
                .iter()
                .map(|request| context.perform(request))
                .collect::<Result<_, _>>()?;
            if matches!(fields.get("done"), Some(Value::Bool(true))) {
                finished = Some(fields.remove("repeat_after"));
                break;
            }
        }
        match finished {
            None => return Err(format!("{} did not finish in {MAX_STEPS} steps", module.id)),
            // Seconds, as a number or as text such as a `--interval` flag.
            Some(Some(Value::Number(seconds))) if seconds > 0.0 => {
                std::thread::sleep(std::time::Duration::from_secs_f64(seconds.min(86_400.0)));
            }
            Some(Some(Value::Text(seconds))) => match seconds.trim().parse::<f64>() {
                Ok(seconds) if seconds > 0.0 => {
                    std::thread::sleep(std::time::Duration::from_secs_f64(seconds.min(86_400.0)));
                }
                _ => {
                    return Err(format!(
                        "{}: repeat_after must be a number of seconds",
                        module.id
                    ));
                }
            },
            Some(_) => return Ok(()),
        }
    }
}

/// `--name value`, `--name=value` and bare `--flag` become `flags`; the rest,
/// in order, `positional`.
fn parse_args(args: &[String]) -> Value {
    let mut flags = BTreeMap::new();
    let mut positional = Vec::new();
    let mut words = args.iter().peekable();
    while let Some(word) = words.next() {
        match word.strip_prefix("--") {
            Some(flag) if !flag.is_empty() => {
                let (name, value) = match flag.split_once('=') {
                    Some((name, value)) => (name, Value::Text(value.into())),
                    None => match words.next_if(|next| !next.starts_with("--")) {
                        Some(value) => (flag, Value::Text(value.clone())),
                        None => (flag, Value::Bool(true)),
                    },
                };
                flags.insert(name.replace('-', "_"), value);
            }
            _ => positional.push(Value::Text(word.clone())),
        }
    }
    lang::eval::modules::record([
        ("flags".into(), Value::Record(flags)),
        ("positional".into(), Value::List(positional)),
    ])
}

struct Context<'a> {
    module: &'a str,
    dir: &'a Path,
}

impl Context<'_> {
    /// One effect. An effect that fails answers `{ok: false, error}`; only a
    /// malformed request stops the run, since that is a bug in the command.
    fn perform(&self, request: &Value) -> Result<Value, String> {
        let request = json(request).map_err(|e| e.to_string())?;
        let kind = request["kind"].as_str().unwrap_or_default();
        let outcome = match kind {
            "http" => self.http(&request),
            "read" => self.path(&request["path"]).and_then(|path| {
                let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                if request["json"].as_bool() == Some(true) {
                    let json: serde_json::Value = serde_json::from_str(&text)
                        .map_err(|e| format!("{}: {e}", path.display()))?;
                    return Ok(serde_json::json!({ "json": json }));
                }
                Ok(serde_json::json!({ "text": text }))
            }),
            "list" => self.path(&request["path"]).and_then(|path| {
                let mut files: Vec<String> = std::fs::read_dir(&path)
                    .map_err(|e| e.to_string())?
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| entry.path().is_file())
                    .map(|entry| entry.file_name().to_string_lossy().into_owned())
                    .collect();
                files.sort();
                Ok(serde_json::json!({ "files": files }))
            }),
            "write" => self.path(&request["path"]).and_then(|path| {
                let text = match (&request["text"], &request["json"]) {
                    (serde_json::Value::String(text), _) => text.clone(),
                    (_, json) if !json.is_null() => {
                        serde_json::to_string_pretty(json).map_err(|e| e.to_string())? + "\n"
                    }
                    _ => return Err("write needs text or json".into()),
                };
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                std::fs::write(&path, text).map_err(|e| e.to_string())?;
                Ok(serde_json::json!({}))
            }),
            "move" => self.path(&request["from"]).and_then(|from| {
                let to = self.path(&request["to"])?;
                if let Some(parent) = to.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                std::fs::rename(&from, &to).map_err(|e| e.to_string())?;
                Ok(serde_json::json!({}))
            }),
            "remove" => self.path(&request["path"]).and_then(|path| {
                std::fs::remove_file(&path).map_err(|e| e.to_string())?;
                Ok(serde_json::json!({}))
            }),
            "credential" => self.credential(&request),
            "env" => {
                let name = request["name"].as_str().unwrap_or_default();
                if !name.starts_with("XMD_") {
                    return Err(format!("{}: only XMD_* variables can be read", self.module));
                }
                Ok(serde_json::json!({
                    "value": std::env::var(name).ok().filter(|value| !value.is_empty())
                }))
            }
            "uuid" => Ok(serde_json::json!({ "value": uuid() })),
            other => return Err(format!("{}: unknown request kind '{other}'", self.module)),
        };
        let mut answer = match outcome {
            Ok(value) => value,
            Err(error) => serde_json::json!({ "ok": false, "error": error }),
        };
        if answer.get("ok").is_none() {
            answer["ok"] = serde_json::Value::Bool(true);
        }
        Ok(from_json(&answer))
    }

    /// A path inside the command's directory; anything leaving it is refused.
    fn path(&self, value: &serde_json::Value) -> Result<PathBuf, String> {
        let relative = value.as_str().ok_or("Expected a path")?;
        let path = Path::new(relative);
        if relative.is_empty()
            || !path
                .components()
                .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
        {
            return Err(format!(
                "{relative}: paths stay inside the command's directory"
            ));
        }
        Ok(self.dir.join(path))
    }

    /// `curl`, as the rest of the command line uses: JSON in, JSON out.
    fn http(&self, request: &serde_json::Value) -> Result<serde_json::Value, String> {
        let method = request["method"].as_str().unwrap_or("GET").to_uppercase();
        if !matches!(method.as_str(), "GET" | "PUT" | "POST" | "PATCH" | "DELETE") {
            return Err(format!("Unsupported method {method}"));
        }
        let url = request["url"].as_str().ok_or("http needs a url")?;
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(format!("{url}: only http and https addresses"));
        }
        let mut command = Command::new("curl");
        command
            .args(["-sS", "-L", "--max-time", "30", "-X", &method, url])
            .args(["-H", "Accept: application/json", "-w", "\n%{http_code}"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(headers) = request["headers"].as_object() {
            for (name, value) in headers {
                let value = value.as_str().ok_or("Header values must be text")?;
                if name.contains([':', '\n', '\r']) || value.contains(['\n', '\r']) {
                    return Err(format!("Invalid header {name}"));
                }
                command.args(["-H", &format!("{name}: {value}")]);
            }
        }
        let body = request.get("json").filter(|body| !body.is_null());
        if body.is_some() {
            command
                .args([
                    "-H",
                    "Content-Type: application/json",
                    "--data-binary",
                    "@-",
                ])
                .stdin(Stdio::piped());
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("Cannot run curl: {e}"))?;
        if let Some(body) = body {
            let bytes = serde_json::to_vec(body).map_err(|e| e.to_string())?;
            child
                .stdin
                .take()
                .ok_or("curl has no input")?
                .write_all(&bytes)
                .map_err(|e| e.to_string())?;
        }
        let output = child.wait_with_output().map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "curl failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        let (body, code) = text.rsplit_once('\n').ok_or("Unexpected response")?;
        let status: u16 = code.trim().parse().unwrap_or(0);
        let mut parsed: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
        if let Some(keep) = request["pick"].as_array()
            && let Some(items) = parsed.as_array_mut()
        {
            let keep: Vec<&str> = keep.iter().filter_map(|k| k.as_str()).collect();
            for item in items {
                if let Some(fields) = item.as_object_mut() {
                    fields.retain(|name, _| keep.contains(&name.as_str()));
                }
            }
        }
        let text = if parsed.is_null() {
            body.chars().take(65_536).collect()
        } else {
            String::new()
        };
        Ok(serde_json::json!({ "status": status, "json": parsed, "text": text }))
    }

    /// A secret saved for this command and scope, such as an API key per address.
    fn credential(&self, request: &serde_json::Value) -> Result<serde_json::Value, String> {
        let scope = request["scope"]
            .as_str()
            .ok_or("credential needs a scope")?;
        let key = format!("{} {scope}", self.module);
        let path = credentials_path().ok_or("No config directory for credentials")?;
        let mut saved: BTreeMap<String, String> = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        if let Some(value) = request["set"].as_str() {
            saved.insert(key.clone(), value.to_string());
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let text = serde_json::to_string_pretty(&saved).map_err(|e| e.to_string())? + "\n";
            std::fs::write(&path, text).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
        Ok(serde_json::json!({ "value": saved.get(&key) }))
    }
}

/// Where saved credentials live: `$XDG_CONFIG_HOME/xmd` or `~/.config/xmd`.
fn credentials_path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(config.join("xmd").join("credentials.json"))
}

/// A random version-4-shaped identifier, without a dependency.
fn uuid() -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut out = String::new();
    for salt in 0..2u64 {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(
            salt ^ std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0),
        );
        out.push_str(&format!("{:016x}", hasher.finish()));
    }
    format!(
        "{}-{}-4{}-a{}-{}",
        &out[..8],
        &out[8..12],
        &out[13..16],
        &out[17..20],
        &out[20..32]
    )
}
