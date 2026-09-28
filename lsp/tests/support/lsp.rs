//! A stdio driver for the real `xmd lsp` binary.
//!
//! It owns the transport only: framing, the initialize handshake, request ids
//! and answering server-initiated requests. Everything about *what* to send
//! lives in `tests/snapshots.rs`, which drives it from a case script.

use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::Duration,
};

/// How long any single message is waited for before the test fails.
pub const TIMEOUT: Duration = Duration::from_secs(20);

/// The capabilities every snapshot case announces, so servers take the richest
/// path through each feature (snippets, hierarchical symbols, refresh, ...).
pub fn capabilities() -> Value {
    json!({
        "textDocument": {
            "completion": {"completionItem": {"snippetSupport": true}},
            "documentSymbol": {"hierarchicalDocumentSymbolSupport": true}
        },
        "window": {"showDocument": {"support": true}},
        "workspace": {
            "inlayHint": {"refreshSupport": true},
            "semanticTokens": {"refreshSupport": true},
            "codeLens": {"refreshSupport": true},
            "didChangeWatchedFiles": {"dynamicRegistration": true}
        }
    })
}

pub struct Lsp {
    child: Child,
    input: ChildStdin,
    output: Receiver<Value>,
    next: u64,
    /// Messages taken from the channel but not consumed yet, so a `request`
    /// never swallows a notification a later `await` is looking for.
    pending: VecDeque<Value>,
    /// The `semanticTokensProvider` legend reported by `initialize`.
    pub token_types: Vec<String>,
    pub token_modifiers: Vec<String>,
    pub initialize_result: Value,
}

impl Lsp {
    /// Starts `xmd lsp` in `root` with the clock frozen at `now` (`XMD_NOW`),
    /// `PATH` set to `path`, and completes the initialize handshake.
    pub fn start(root: &Path, now: &str, path: &str) -> Self {
        Self::start_with(root, now, path, capabilities())
    }

    /// Like `start`, but announcing the given client capabilities, so a case can
    /// take the path a poorer editor takes.
    pub fn start_with(root: &Path, now: &str, path: &str, capabilities: Value) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_xmd"))
            .arg("lsp")
            .current_dir(root)
            .env("XMD_NOW", now)
            .env("TZ", "UTC")
            .env("PATH", path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("failed to start `xmd lsp`");
        let input = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, output) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut length = 0usize;
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap_or(0) == 0 {
                        return;
                    }
                    if header.trim().is_empty() {
                        break;
                    }
                    if let Some(n) = header.strip_prefix("Content-Length:") {
                        length = n.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; length];
                if reader.read_exact(&mut body).is_err() {
                    return;
                }
                if sender.send(serde_json::from_slice(&body).unwrap()).is_err() {
                    return;
                }
            }
        });
        let mut client = Self {
            child,
            input,
            output,
            next: 0,
            pending: VecDeque::new(),
            token_types: Vec::new(),
            token_modifiers: Vec::new(),
            initialize_result: Value::Null,
        };
        let root_uri = url::Url::from_directory_path(root).unwrap();
        let result = client.request(
            "initialize",
            json!({"processId": null, "rootUri": root_uri, "capabilities": capabilities}),
        );
        let legend = &result["capabilities"]["semanticTokensProvider"]["legend"];
        client.token_types = strings(&legend["tokenTypes"]);
        client.token_modifiers = strings(&legend["tokenModifiers"]);
        client.initialize_result = result;
        client.notify("initialized", json!({}));
        client
    }

    fn send(&mut self, value: Value) {
        let body = serde_json::to_vec(&value).unwrap();
        write!(self.input, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        self.input.write_all(&body).unwrap();
        self.input.flush().unwrap();
    }

    pub fn notify(&mut self, method: &str, params: Value) {
        let mut message = json!({"jsonrpc": "2.0", "method": method});
        if !params.is_null() {
            message["params"] = params;
        }
        self.send(message);
    }

    /// The next message: a held one first, otherwise the next from the server.
    /// Server-initiated requests are answered the moment they are read.
    pub fn take(&mut self) -> Value {
        if let Some(value) = self.pending.pop_front() {
            return value;
        }
        let value = self
            .output
            .recv_timeout(TIMEOUT)
            .expect("the language server went quiet");
        if value.get("id").is_some() && value.get("method").is_some() {
            let result = match value["method"].as_str() {
                Some("workspace/applyEdit") => json!({"applied": true}),
                Some("window/showDocument") => json!({"success": true}),
                _ => Value::Null,
            };
            self.send(json!({"jsonrpc": "2.0", "id": value["id"], "result": result}));
        }
        value
    }

    /// Puts messages back at the front of the queue, keeping their order.
    pub fn hold(&mut self, messages: Vec<Value>) {
        for message in messages.into_iter().rev() {
            self.pending.push_front(message);
        }
    }

    /// Sends a request and returns the whole response message. Everything that
    /// arrives in the meantime is held for later steps.
    pub fn request_message(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        let mut message = json!({"jsonrpc": "2.0", "id": id, "method": method});
        if !params.is_null() {
            message["params"] = params;
        }
        self.send(message);
        let mut held = Vec::new();
        let response = loop {
            let value = self.take();
            if value["id"] == json!(id) && value.get("method").is_none() {
                break value;
            }
            held.push(value);
        };
        self.pending.extend(held);
        response
    }

    pub fn request(&mut self, method: &str, params: Value) -> Value {
        let response = self.request_message(method, params);
        assert!(response.get("error").is_none(), "{method}: {response}");
        response["result"].clone()
    }
}

impl Drop for Lsp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|v| v.as_str().unwrap_or_default().to_owned())
                .collect()
        })
        .unwrap_or_default()
}
