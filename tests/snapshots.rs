//! The end-to-end snapshot suite.
//!
//! Every case is a directory under `tests/cases/`. The harness copies it into a
//! temporary workspace, replays the steps in `case.json` against the real
//! binary (CLI, language server) or the browser host, and compares the recorded
//! transcript with `expected.snap`.
//!
//! See `tests/README.md` for the case layout and the step vocabulary.
//! Run one case with `SNAPSHOT_CASE=<name>`, rewrite snapshots with
//! `UPDATE_SNAPSHOTS=1`.
#![cfg(feature = "native")]

mod support;

use serde_json::{Map, Value, json};
use std::{
    collections::BTreeMap,
    fmt::Write as _,
    path::{Path, PathBuf},
    process::Command,
};
use support::lsp::Lsp;

const DEFAULT_NOW: &str = "2026-09-16T14:00:00-04:00";
/// Subcommands that take `--root`, and the subset that also takes `--now`.
const ROOTED: [&str; 5] = ["query", "render", "ast", "graph", "refresh"];
const CLOCKED: [&str; 4] = ["query", "render", "ast", "graph"];

fn cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cases")
}

#[test]
fn snapshots() {
    let only = std::env::var("SNAPSHOT_CASE").ok();
    let update = std::env::var("UPDATE_SNAPSHOTS").is_ok_and(|v| !v.is_empty() && v != "0");
    let mut names: Vec<String> = std::fs::read_dir(cases_dir())
        .expect("tests/cases is missing")
        .map(|entry| entry.unwrap())
        .filter(|entry| entry.file_type().unwrap().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    if let Some(only) = &only {
        names.retain(|name| name == only);
        assert!(!names.is_empty(), "No case named {only} in tests/cases");
    }
    assert!(!names.is_empty(), "No snapshot cases found");

    let mut failures = Vec::new();
    for name in &names {
        match run_case(name, update) {
            Outcome::Skipped(why) => println!("skip {name} ({why})"),
            Outcome::Updated => println!("update {name}"),
            Outcome::Passed => println!("ok {name}"),
            Outcome::Failed(report) => {
                println!("FAIL {name}");
                failures.push(format!("\n===== {name} =====\n{report}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} snapshot case(s) failed:\n{}\nRerun with UPDATE_SNAPSHOTS=1 to accept, \
         SNAPSHOT_CASE=<name> to isolate.",
        failures.len(),
        failures.join("\n")
    );
}

enum Outcome {
    Passed,
    Updated,
    Skipped(&'static str),
    Failed(String),
}

fn run_case(name: &str, update: bool) -> Outcome {
    let dir = cases_dir().join(name);
    let script: Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("case.json"))
            .unwrap_or_else(|e| panic!("{name}/case.json: {e}")),
    )
    .unwrap_or_else(|e| panic!("{name}/case.json: {e}"));
    if script["requires"] == json!("browser") && !cfg!(feature = "browser") {
        return Outcome::Skipped("requires --features browser");
    }
    let now = script["now"].as_str().unwrap_or(DEFAULT_NOW).to_owned();

    let temp = tempfile::tempdir().unwrap();
    let raw_root = temp.path().to_path_buf();
    copy_case(&dir, &raw_root);
    let root = raw_root.canonicalize().unwrap();
    let mut world = World::new(&root, &raw_root, &now);

    let mut transcript = String::new();
    let steps = script["steps"].as_array().cloned().unwrap_or_default();
    for (index, step) in steps.iter().enumerate() {
        world.run_step(index, step, &mut transcript);
    }
    if !transcript.ends_with('\n') {
        transcript.push('\n');
    }

    let expected_path = dir.join("expected.snap");
    if update {
        std::fs::write(&expected_path, &transcript).unwrap();
        return Outcome::Updated;
    }
    let expected = std::fs::read_to_string(&expected_path).unwrap_or_default();
    if expected == transcript {
        Outcome::Passed
    } else {
        let path = root.display().to_string();
        // Keep the workspace around so the diff can be inspected by hand.
        let kept = temp.keep();
        Outcome::Failed(format!(
            "{}\nworkspace: {} (kept at {})\n",
            diff(&expected, &transcript),
            path,
            kept.display()
        ))
    }
}

/// Copies everything but the script and the snapshot, making `bin/` executable.
fn copy_case(from: &Path, to: &Path) {
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name == "case.json" || name == "expected.snap" {
            continue;
        }
        copy_tree(&entry.path(), &to.join(&name));
    }
}

fn copy_tree(from: &Path, to: &Path) {
    if from.is_dir() {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            copy_tree(&entry.path(), &to.join(entry.file_name()));
        }
        return;
    }
    std::fs::create_dir_all(to.parent().unwrap()).unwrap();
    std::fs::copy(from, to).unwrap();
    #[cfg(unix)]
    if to.parent().is_some_and(|p| p.ends_with("bin")) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(to, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

// ---------------------------------------------------------------- the world

struct World {
    root: PathBuf,
    raw_root: PathBuf,
    now: String,
}

impl World {
    fn new(root: &Path, raw_root: &Path, now: &str) -> Self {
        Self {
            root: root.into(),
            raw_root: raw_root.into(),
            now: now.into(),
        }
    }

    /// Replaces every spelling of the temporary workspace with `<root>`, and
    /// any sub-second timestamp with `<clock>`.
    fn scrub(&self, text: &str) -> String {
        let mut out = text.to_owned();
        for root in [&self.root, &self.raw_root] {
            let root = root.display().to_string();
            if !root.is_empty() {
                out = out.replace(&root, "<root>");
            }
        }
        undate(&out)
    }

    /// Raw note text goes into the transcript with carriage returns made
    /// visible, so CRLF notes cannot silently become LF ones.
    fn body(&self, text: &str) -> String {
        self.scrub(text).replace('\r', "\u{240d}")
    }

    fn scrub_json(&self, value: &Value) -> Value {
        match value {
            Value::String(s) => Value::String(self.scrub(s)),
            Value::Array(items) => Value::Array(items.iter().map(|v| self.scrub_json(v)).collect()),
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(k, v)| (self.scrub(k), self.scrub_json(v)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    /// `bin/` in the case directory shadows real programs, so refresh
    /// providers (`gh`, ...) can be faked with shell scripts.
    fn path(&self) -> String {
        let system = std::env::var("PATH").unwrap_or_default();
        let bin = self.root.join("bin");
        if bin.is_dir() {
            format!("{}:{system}", bin.display())
        } else {
            system
        }
    }

    fn uri(&self, file: &str) -> String {
        url::Url::from_file_path(self.root.join(file))
            .unwrap()
            .to_string()
    }

    fn run_step(&mut self, index: usize, step: &Value, out: &mut String) {
        if let Some(args) = step.get("cli").and_then(Value::as_array) {
            let shown: Vec<String> = args.iter().map(as_text).collect();
            let root = self.root.display().to_string();
            let args: Vec<String> = shown.iter().map(|a| a.replace("${root}", &root)).collect();
            let _ = writeln!(out, "### {index} cli {}", shown.join(" "));
            self.cli(&args, out);
        } else if let Some(items) = step.get("lsp").and_then(Value::as_array) {
            let _ = writeln!(out, "### {index} lsp");
            self.lsp(items, out);
        } else if let Some(paths) = step.get("read").and_then(Value::as_array) {
            let paths: Vec<String> = paths.iter().map(as_text).collect();
            let _ = writeln!(out, "### {index} read {}", paths.join(" "));
            for path in &paths {
                let _ = writeln!(out, "-- {path}");
                match std::fs::read(self.root.join(path)) {
                    Ok(bytes) => {
                        let text = self.body(&String::from_utf8_lossy(&bytes));
                        out.push_str(&text);
                        if !text.ends_with('\n') {
                            out.push('\n');
                        }
                    }
                    Err(e) => {
                        let _ = writeln!(out, "(missing: {})", e.kind());
                    }
                }
            }
        } else if let Some(items) = step.get("browser").and_then(Value::as_array) {
            let _ = writeln!(out, "### {index} browser");
            self.browser(items, out);
        } else {
            panic!("Unknown step kind: {step}");
        }
        out.push('\n');
    }

    // ------------------------------------------------------------------ cli

    fn cli(&self, args: &[String], out: &mut String) {
        let subcommand = args.first().map(String::as_str).unwrap_or_default();
        let mut full: Vec<String> = args.to_vec();
        let has = |flag: &str| args.iter().any(|a| a == flag);
        if ROOTED.contains(&subcommand) && !has("--root") {
            full.push("--root".into());
            full.push(self.root.display().to_string());
        }
        if CLOCKED.contains(&subcommand) && !has("--now") && !has("--on") {
            full.push("--now".into());
            full.push(self.now.clone());
        }
        let output = Command::new(env!("CARGO_BIN_EXE_wtf"))
            .current_dir(&self.root)
            .env("TZ", "UTC")
            .env("WTF_NOW", &self.now)
            .env("PATH", self.path())
            .args(&full)
            .output()
            .expect("failed to run the wtf binary");
        let shown: Vec<String> = full.iter().map(|a| self.scrub(a)).collect();
        let _ = writeln!(out, "$ wtf {}", shown.join(" "));
        let _ = writeln!(
            out,
            "exit {}",
            output
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".into())
        );
        for (name, bytes) in [("stdout", &output.stdout), ("stderr", &output.stderr)] {
            if bytes.is_empty() {
                continue;
            }
            let text = self.body(&String::from_utf8_lossy(bytes));
            let _ = writeln!(out, "--- {name}");
            out.push_str(&text);
            if !text.ends_with('\n') {
                out.push('\n');
            }
        }
    }

    // ------------------------------------------------------------------ lsp

    fn lsp(&self, items: &[Value], out: &mut String) {
        let mut lsp = Lsp::start(&self.root, &self.now, &self.path());
        let mut texts: BTreeMap<String, String> = BTreeMap::new();
        let mut versions: BTreeMap<String, i64> = BTreeMap::new();
        let mut last = Value::Null;
        let mut shown;
        for item in items {
            let file = item.get("file").map(as_text);
            if let Some(name) = item.get("open").map(as_text) {
                let text = std::fs::read_to_string(self.root.join(&name))
                    .unwrap_or_else(|e| panic!("open {name}: {e}"));
                versions.insert(name.clone(), 1);
                texts.insert(name.clone(), text.clone());
                lsp.notify(
                    "textDocument/didOpen",
                    json!({"textDocument":{"uri":self.uri(&name),"languageId":"wtf","version":1,"text":text}}),
                );
                let _ = writeln!(out, "-- open {name}");
            } else if let Some(change) = item.get("change") {
                let name = as_text(&change["file"]);
                let text = as_text(&change["text"]);
                let version = versions.entry(name.clone()).or_insert(1);
                *version += 1;
                texts.insert(name.clone(), text.clone());
                lsp.notify(
                    "textDocument/didChange",
                    json!({"textDocument":{"uri":self.uri(&name),"version":*version},"contentChanges":[{"text":text}]}),
                );
                let _ = writeln!(out, "-- change {name} (version {version})");
                let shown = self.body(&text);
                out.push_str(&shown);
                if !shown.ends_with('\n') {
                    out.push('\n');
                }
            } else if let Some(name) = item.get("save").map(as_text) {
                lsp.notify(
                    "textDocument/didSave",
                    json!({"textDocument":{"uri":self.uri(&name)}}),
                );
                let _ = writeln!(out, "-- save {name}");
            } else if let Some(name) = item.get("close").map(as_text) {
                lsp.notify(
                    "textDocument/didClose",
                    json!({"textDocument":{"uri":self.uri(&name)}}),
                );
                let _ = writeln!(out, "-- close {name}");
            } else if let Some(query) = item.get("query") {
                let mut params = query.clone();
                if let Some(uri) = params.get("uri").map(as_text) {
                    params["uri"] = json!(self.uri(&uri));
                }
                params["now"] = json!(self.now);
                let result = lsp.request("wtf/query", params);
                let _ = writeln!(out, "-- query {}", as_text(&query["query"]));
                last = result;
                shown = self.scrub_json(&last);
                out.push_str(&pretty(&shown));
            } else if let Some(method) = item.get("await").map(as_text) {
                let params = self.wait(&mut lsp, &method, file.as_deref());
                let _ = writeln!(
                    out,
                    "-- await {method}{}",
                    file.map(|f| format!(" {f}")).unwrap_or_default()
                );
                last = params;
                shown = self.scrub_json(&last);
                out.push_str(&pretty(&shown));
            } else if let Some(method) = item.get("request").map(as_text) {
                let params = self.params(item, &last);
                let response = lsp.request_message(&method, params);
                let _ = writeln!(out, "-- request {method}{}", describe(item));
                let body = if let Some(error) = response.get("error") {
                    last = Value::Null;
                    shown = self.scrub_json(error);
                    format!("error: {}", pretty(&shown))
                } else {
                    last = response["result"].clone();
                    shown = self.scrub_json(&last);
                    let result = &shown;
                    match method.as_str() {
                        "textDocument/semanticTokens/full" => {
                            let name = file.clone().unwrap_or_default();
                            let text = texts
                                .get(&name)
                                .cloned()
                                .or_else(|| std::fs::read_to_string(self.root.join(&name)).ok());
                            decode_tokens(&lsp, &response["result"], text.as_ref())
                        }
                        _ => pretty(result),
                    }
                };
                out.push_str(&body);
                if item.get("apply") == Some(&json!(true)) {
                    let name = file.expect("apply needs a file");
                    let text = texts.get(&name).cloned().expect("apply needs an open file");
                    let edits: Vec<lsp_types::TextEdit> =
                        serde_json::from_value(response["result"].clone()).unwrap_or_default();
                    let updated = wtf::actions::apply_edits(&text, &edits).expect("edits apply");
                    let version = versions.entry(name.clone()).or_insert(1);
                    *version += 1;
                    texts.insert(name.clone(), updated.clone());
                    lsp.notify(
                        "textDocument/didChange",
                        json!({"textDocument":{"uri":self.uri(&name),"version":*version},"contentChanges":[{"text":updated}]}),
                    );
                    let _ = writeln!(out, "-- applied to {name} (version {version})");
                    let shown = self.body(&updated);
                    out.push_str(&shown);
                    if !shown.ends_with('\n') {
                        out.push('\n');
                    }
                }
            } else {
                panic!("Unknown lsp item: {item}");
            }
        }
    }

    /// Builds request params: `file` becomes `textDocument`, `line`/`character`
    /// become `position`, `range` and `extra` are passed through, and an
    /// explicit `params` object wins (after placeholder substitution).
    fn params(&self, item: &Value, last: &Value) -> Value {
        if let Some(raw) = item.get("params") {
            return self.substitute(raw, last);
        }
        let mut params = Map::new();
        if let Some(file) = item.get("file").map(as_text) {
            params.insert("textDocument".into(), json!({"uri": self.uri(&file)}));
        }
        if item.get("line").is_some() {
            params.insert(
                "position".into(),
                json!({
                    "line": item["line"].as_u64().unwrap_or(0),
                    "character": item.get("character").and_then(Value::as_u64).unwrap_or(0)
                }),
            );
        }
        if let Some(range) = item.get("range") {
            params.insert("range".into(), range.clone());
        }
        if let Some(Value::Object(extra)) = item.get("extra") {
            for (key, value) in extra {
                params.insert(key.clone(), self.substitute(value, last));
            }
        }
        Value::Object(params)
    }

    /// `${uri:note.wtf}` becomes a file URI; `${last}` and `${last.N}` reuse
    /// the previous recorded result (whole, or its Nth array element).
    fn substitute(&self, value: &Value, last: &Value) -> Value {
        match value {
            Value::String(text) => {
                if let Some(file) = text
                    .strip_prefix("${uri:")
                    .and_then(|rest| rest.strip_suffix('}'))
                {
                    return json!(self.uri(file));
                }
                if text == "${last}" {
                    return last.clone();
                }
                if let Some(index) = text
                    .strip_prefix("${last.")
                    .and_then(|rest| rest.strip_suffix('}'))
                    .and_then(|n| n.parse::<usize>().ok())
                {
                    return last.get(index).cloned().unwrap_or(Value::Null);
                }
                Value::String(text.replace("${root}", &self.root.display().to_string()))
            }
            Value::Array(items) => {
                Value::Array(items.iter().map(|v| self.substitute(v, last)).collect())
            }
            Value::Object(fields) => Value::Object(
                fields
                    .iter()
                    .map(|(k, v)| (k.clone(), self.substitute(v, last)))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    /// Waits for the next notification, or server-initiated request, with this
    /// method (and uri, when a file is given), answering anything else.
    fn wait(&self, lsp: &mut Lsp, method: &str, file: Option<&str>) -> Value {
        let uri = file.map(|f| self.uri(f));
        let deadline = std::time::Instant::now() + support::lsp::TIMEOUT;
        let mut held = Vec::new();
        loop {
            assert!(
                std::time::Instant::now() < deadline,
                "no {method} for {uri:?} arrived"
            );
            let message = lsp.take();
            let matches = message["method"] == json!(method)
                && uri.as_ref().is_none_or(|uri| {
                    let params = &message["params"];
                    params["uri"] == json!(uri) || params["textDocument"]["uri"] == json!(uri)
                });
            if matches {
                lsp.hold(held);
                return message["params"].clone();
            }
            held.push(message);
        }
    }

    // -------------------------------------------------------------- browser

    #[cfg(feature = "browser")]
    fn browser(&self, items: &[Value], out: &mut String) {
        let mut host = wtf::browser::BrowserWorkspace::new();
        // Every note in the case starts out loaded, mirroring a live editor.
        let mut notes: Vec<PathBuf> = Vec::new();
        collect_notes(&self.root, &mut notes);
        notes.sort();
        for note in &notes {
            let name = note.strip_prefix(&self.root).unwrap().display().to_string();
            let uri = format!("file:///workspace/{}", name.replace('\\', "/"));
            let text = std::fs::read_to_string(note).unwrap();
            let payload = json!({"uri": uri, "text": text, "version": 1}).to_string();
            host.request("setDocument", &payload, &self.now);
            let _ = writeln!(out, "-- setDocument {name}");
        }
        for item in items {
            let method = as_text(&item["method"]);
            let params = item.get("params").cloned().unwrap_or(json!({}));
            let raw = host.request(&method, &params.to_string(), &self.now);
            let value: Value = serde_json::from_str(&raw).unwrap_or(Value::String(raw));
            let _ = writeln!(out, "-- {method} {}", compact(&params));
            out.push_str(&pretty(&self.scrub_json(&value)));
        }
    }

    #[cfg(not(feature = "browser"))]
    fn browser(&self, _items: &[Value], out: &mut String) {
        out.push_str("(browser steps require --features browser)\n");
    }
}

#[cfg(feature = "browser")]
fn collect_notes(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            if entry.file_name() != ".wtf" && entry.file_name() != "bin" {
                collect_notes(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "wtf") {
            out.push(path);
        }
    }
}

/// Some paths (a `refresh` writing `fetched_at`) stamp the real wall clock,
/// which no frozen clock can reach. Those timestamps always carry fractional
/// seconds, so they are recognisable and become `<clock>`; the whole-second
/// timestamps a case pins down itself are left alone.
fn undate(text: &str) -> String {
    let bytes = text.as_bytes();
    let digits = |at: usize, n: usize| {
        at + n <= bytes.len() && bytes[at..at + n].iter().all(u8::is_ascii_digit)
    };
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        // YYYY-MM-DDTHH:MM:SS.fff...
        let shaped = digits(i, 4)
            && bytes.get(i + 4) == Some(&b'-')
            && digits(i + 5, 2)
            && bytes.get(i + 7) == Some(&b'-')
            && digits(i + 8, 2)
            && bytes.get(i + 10) == Some(&b'T')
            && digits(i + 11, 2)
            && bytes.get(i + 13) == Some(&b':')
            && digits(i + 14, 2)
            && bytes.get(i + 16) == Some(&b':')
            && digits(i + 17, 2)
            && bytes.get(i + 19) == Some(&b'.')
            && digits(i + 20, 1);
        if !shaped {
            let char_len = text[i..].chars().next().map(char::len_utf8).unwrap_or(1);
            out.push_str(&text[i..i + char_len]);
            i += char_len;
            continue;
        }
        let mut end = i + 20;
        while digits(end, 1) {
            end += 1;
        }
        if bytes.get(end) == Some(&b'Z') {
            end += 1;
        } else if matches!(bytes.get(end), Some(b'+') | Some(b'-')) && digits(end + 1, 2) {
            end += 3;
            if bytes.get(end) == Some(&b':') && digits(end + 1, 2) {
                end += 3;
            }
        }
        out.push_str("<clock>");
        i = end;
    }
    out
}

// --------------------------------------------------------------- formatting

fn as_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

fn describe(item: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(file) = item.get("file") {
        parts.push(as_text(file));
    }
    if let Some(line) = item.get("line") {
        parts.push(format!(
            "{}:{}",
            as_text(line),
            item.get("character").map(as_text).unwrap_or("0".into())
        ));
    }
    if let Some(range) = item.get("range") {
        parts.push(compact(range));
    }
    if item.get("params").is_some() {
        parts.push(compact(&item["params"]));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" {}", parts.join(" "))
    }
}

fn compact(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn pretty(value: &Value) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
    text.push('\n');
    text
}

/// Semantic tokens are delta-encoded integers; a snapshot is only useful if it
/// reads back as the text that was coloured.
fn decode_tokens(lsp: &Lsp, result: &Value, text: Option<&String>) -> String {
    let Some(data) = result["data"].as_array() else {
        return pretty(result);
    };
    let lines: Vec<&str> = text.map(|t| t.lines().collect()).unwrap_or_default();
    let mut out = String::new();
    let (mut line, mut start) = (0u32, 0u32);
    for token in data.chunks(5) {
        let [delta_line, delta_start, length, kind, modifiers] = token else {
            break;
        };
        let delta_line = delta_line.as_u64().unwrap_or(0) as u32;
        line += delta_line;
        start = if delta_line == 0 {
            start + delta_start.as_u64().unwrap_or(0) as u32
        } else {
            delta_start.as_u64().unwrap_or(0) as u32
        };
        let length = length.as_u64().unwrap_or(0) as u32;
        let kind = lsp
            .token_types
            .get(kind.as_u64().unwrap_or(0) as usize)
            .cloned()
            .unwrap_or_else(|| "?".into());
        let bits = modifiers.as_u64().unwrap_or(0);
        let names: Vec<&str> = lsp
            .token_modifiers
            .iter()
            .enumerate()
            .filter(|(i, _)| bits & (1 << i) != 0)
            .map(|(_, name)| name.as_str())
            .collect();
        let slice = lines
            .get(line as usize)
            .and_then(|text| {
                let from = wtf::document::byte_at(text, start)?;
                let to = wtf::document::byte_at(text, start + length)?;
                Some(text[from..to].to_owned())
            })
            .unwrap_or_else(|| "<out of range>".into());
        let _ = writeln!(
            out,
            "{line}:{start}+{length} {kind}{} {:?}",
            if names.is_empty() {
                String::new()
            } else {
                format!(" [{}]", names.join(","))
            },
            slice
        );
    }
    if out.is_empty() {
        out.push_str("(no tokens)\n");
    }
    out
}

// --------------------------------------------------------------------- diff

/// A minimal unified line diff, so a failure reads without extra dependencies.
fn diff(expected: &str, actual: &str) -> String {
    let a: Vec<&str> = expected.lines().collect();
    let b: Vec<&str> = actual.lines().collect();
    let mut prefix = 0;
    while prefix < a.len() && prefix < b.len() && a[prefix] == b[prefix] {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < a.len() - prefix
        && suffix < b.len() - prefix
        && a[a.len() - 1 - suffix] == b[b.len() - 1 - suffix]
    {
        suffix += 1;
    }
    let mut out = String::from("--- expected\n+++ actual\n");
    let _ = writeln!(out, "@@ line {} @@", prefix + 1);
    for line in a.iter().skip(prefix).take(a.len() - suffix - prefix) {
        let _ = writeln!(out, "-{line}");
    }
    for line in b.iter().skip(prefix).take(b.len() - suffix - prefix) {
        let _ = writeln!(out, "+{line}");
    }
    if a.len() == prefix + suffix && b.len() == prefix + suffix {
        out.push_str("(only trailing whitespace differs)\n");
    }
    out
}
