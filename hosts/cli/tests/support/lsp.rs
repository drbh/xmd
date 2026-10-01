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
pub(crate) const TIMEOUT: Duration = Duration::from_secs(20);

/// The capabilities every snapshot case announces, so servers take the richest
/// path through each feature (snippets, hierarchical symbols, refresh, ...).
pub(crate) fn capabilities() -> Value {
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

pub(crate) struct Lsp {
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
    ///
    /// Given client capabilities stand in for [`capabilities`], so a case can
    /// take the path a poorer editor takes; a `folder` is the one workspace
    /// folder announced, under `root`, which may be a single note the way Zed
    /// opens one file.
    pub(crate) fn start(
        root: &Path,
        now: &str,
        path: &str,
        capabilities: Option<Value>,
        folder: Option<&Path>,
    ) -> Self {
        let capabilities = capabilities.unwrap_or_else(self::capabilities);
        let mut command = Command::new(env!("CARGO_BIN_EXE_xmd"));
        super::isolate(&mut command, &root.join("config"), now);
        let mut child = command
            .arg("lsp")
            .current_dir(root)
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
        let mut params =
            json!({"processId": null, "rootUri": root_uri, "capabilities": capabilities});
        if let Some(folder) = folder {
            let uri = url::Url::from_file_path(folder).unwrap();
            params["workspaceFolders"] = json!([{"uri": uri, "name": "folder"}]);
        }
        let result = client.request("initialize", params);
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

    pub(crate) fn notify(&mut self, method: &str, params: Value) {
        self.send(message(method, params));
    }

    /// The next message: a held one first, otherwise the next from the server.
    /// Server-initiated requests are answered the moment they are read.
    pub(crate) fn take(&mut self) -> Value {
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
    pub(crate) fn hold(&mut self, messages: Vec<Value>) {
        for message in messages.into_iter().rev() {
            self.pending.push_front(message);
        }
    }

    /// Sends a request and returns the whole response message. Everything that
    /// arrives in the meantime is held for later steps.
    pub(crate) fn request_message(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        let mut message = message(method, params);
        message["id"] = json!(id);
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

    pub(crate) fn request(&mut self, method: &str, params: Value) -> Value {
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

/// A JSON-RPC message for `method`, with `params` unless they are null.
fn message(method: &str, params: Value) -> Value {
    let mut message = json!({"jsonrpc": "2.0", "method": method});
    if !params.is_null() {
        message["params"] = params;
    }
    message
}

fn strings(value: &Value) -> Vec<String> {
    let items = value.as_array().into_iter().flatten();
    items
        .map(|v| v.as_str().unwrap_or_default().to_owned())
        .collect()
}
