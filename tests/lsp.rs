use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::Duration,
};
use tower_lsp::lsp_types::{TextEdit, Url};

struct Lsp {
    child: Child,
    input: ChildStdin,
    output: Receiver<Value>,
    next: u64,
    requests: Vec<String>,
    applied_edits: Vec<Value>,
}
impl Lsp {
    fn start(root: &std::path::Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_jot"))
            .arg("lsp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (sender, output) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut length = 0;
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
            requests: Vec::new(),
            applied_edits: Vec::new(),
        };
        let result=client.request("initialize",json!({"processId":null,"rootUri":Url::from_directory_path(root).unwrap(),"capabilities":{"workspace":{"inlayHint":{"refreshSupport":true},"semanticTokens":{"refreshSupport":true},"didChangeWatchedFiles":{"dynamicRegistration":true}}}}));
        assert_eq!(result["capabilities"]["inlayHintProvider"], true);
        assert_eq!(result["capabilities"]["renameProvider"], true);
        assert!(
            result["capabilities"]["semanticTokensProvider"]["legend"]["tokenTypes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t == "variable")
        );
        client.notify("initialized", json!({}));
        client
    }
    fn send(&mut self, value: Value) {
        let body = serde_json::to_vec(&value).unwrap();
        write!(self.input, "Content-Length: {}\r\n\r\n", body.len()).unwrap();
        self.input.write_all(&body).unwrap();
        self.input.flush().unwrap();
    }
    fn notify(&mut self, method: &str, params: Value) {
        let mut message = json!({"jsonrpc":"2.0","method":method});
        if !params.is_null() {
            message["params"] = params;
        }
        self.send(message);
    }
    fn receive(&mut self) -> Value {
        loop {
            let value = self
                .output
                .recv_timeout(Duration::from_secs(10))
                .expect("language server timed out");
            if !self.handle_server_request(&value) {
                return value;
            }
        }
    }
    fn handle_server_request(&mut self, value: &Value) -> bool {
        if let (Some(id), Some(method)) = (value.get("id"), value["method"].as_str()) {
            self.requests.push(method.into());
            let result = if method == "workspace/applyEdit" {
                self.applied_edits.push(value["params"]["edit"].clone());
                json!({"applied": true})
            } else {
                Value::Null
            };
            self.send(json!({"jsonrpc":"2.0","id":id,"result":result}));
            true
        } else {
            false
        }
    }
    fn wait_for_request(&mut self, method: &str) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let value = self
                .output
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("No live refresh from server");
            if self.handle_server_request(&value) && value["method"] == method {
                return;
            }
        }
    }
    fn timer_action(
        &mut self,
        uri: &Url,
        text: &str,
        row: u32,
        version: u32,
        action: &str,
    ) -> String {
        let response = self.request("textDocument/codeAction", json!({"textDocument":{"uri":uri},"range":{"start":{"line":row,"character":0},"end":{"line":row,"character":0}},"context":{"diagnostics":[]}}));
        let command = response
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["title"].as_str().is_some_and(|s| s.starts_with(action)))
            .expect("Timer action missing");
        self.request(
            "workspace/executeCommand",
            json!({"command":command["command"],"arguments":command["arguments"]}),
        );
        let edit = self
            .applied_edits
            .pop()
            .expect("Timer did not request an editor edit");
        assert_eq!(
            edit["documentChanges"][0]["textDocument"]["version"],
            version
        );
        let edits: Vec<TextEdit> =
            serde_json::from_value(edit["documentChanges"][0]["edits"].clone()).unwrap();
        let text = jot::actions::apply_edits(text, &edits).unwrap();
        self.notify("textDocument/didChange", json!({"textDocument":{"uri":uri,"version":version + 1},"contentChanges":[{"text":text}]}));
        assert_eq!(self.diagnostics(version as i32 + 1), json!([]));
        text
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        let mut message = json!({"jsonrpc":"2.0","id":id,"method":method});
        if !params.is_null() {
            message["params"] = params;
        }
        self.send(message);
        loop {
            let v = self.receive();
            if v["id"] == id {
                assert!(v.get("error").is_none(), "{v}");
                return v["result"].clone();
            }
        }
    }
    fn diagnostics(&mut self, version: i32) -> Value {
        loop {
            let v = self.receive();
            if v["method"] == "textDocument/publishDiagnostics" && v["params"]["version"] == version
            {
                return v["params"]["diagnostics"].clone();
            }
        }
    }
}
impl Drop for Lsp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn zed_workflow_updates_hints_highlights_links_tasks_and_cross_file_names() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let resource = root.join("resources.jot");
    std::fs::write(
        &resource,
        "[./receipt.png]:receipt\n[2026-09-25]:departure\n",
    )
    .unwrap();
    let path = root.join("daily.jot");
    let uri = Url::from_file_path(&path).unwrap();
    let text = "# Release :release\n- [x] Parser\n- [ ] Review [receipt] :review @estimate(30m)\n- [ ] Publish @after(review) @due(departure-7d)\n[progress] := completed(release)/total(release)\n";
    std::fs::write(&path, text).unwrap();
    let mut lsp = Lsp::start(&root);
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"jot","version":1,"text":text}}),
    );
    assert_eq!(lsp.diagnostics(1), json!([]));
    let range = json!({"start":{"line":0,"character":0},"end":{"line":5,"character":0}});
    let hints = lsp.request(
        "textDocument/inlayHint",
        json!({"textDocument":{"uri":uri},"range":range}),
    );
    assert!(hints.to_string().contains("1/3 complete"));
    assert!(hints.to_string().contains("blocked by review"));
    let tokens = lsp.request(
        "textDocument/semanticTokens/full",
        json!({"textDocument":{"uri":uri}}),
    );
    assert!(!tokens["data"].as_array().unwrap().is_empty());
    let hover = lsp.request(
        "textDocument/hover",
        json!({"textDocument":{"uri":uri},"position":{"line":2,"character":16}}),
    );
    assert!(hover.to_string().contains("receipt.png"));
    let definition = lsp.request(
        "textDocument/definition",
        json!({"textDocument":{"uri":uri},"position":{"line":2,"character":16}}),
    );
    assert_eq!(
        definition["uri"],
        Url::from_file_path(&resource).unwrap().as_str()
    );
    let links = lsp.request(
        "textDocument/documentLink",
        json!({"textDocument":{"uri":uri}}),
    );
    assert!(links.to_string().contains("receipt.png"));
    let rename=lsp.request("textDocument/rename",json!({"textDocument":{"uri":uri},"position":{"line":2,"character":16},"newName":"lunch_receipt"}));
    assert_eq!(rename["documentChanges"].as_array().unwrap().len(), 2);
    assert!(rename.to_string().contains("lunch_receipt"));
    let actions=lsp.request("textDocument/codeAction",json!({"textDocument":{"uri":uri},"range":{"start":{"line":2,"character":0},"end":{"line":2,"character":0}},"context":{"diagnostics":[]}}));
    let complete = actions
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["title"] == "Complete task")
        .unwrap();
    assert_eq!(
        complete["edit"]["documentChanges"][0]["textDocument"]["version"],
        1
    );
    let edits: Vec<TextEdit> =
        serde_json::from_value(complete["edit"]["documentChanges"][0]["edits"].clone()).unwrap();
    let changed = jot::actions::apply_edits(text, &edits).unwrap();
    lsp.notify(
        "textDocument/didChange",
        json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":changed}]}),
    );
    assert_eq!(lsp.diagnostics(2), json!([]));
    let hints = lsp.request(
        "textDocument/inlayHint",
        json!({"textDocument":{"uri":uri},"range":range}),
    );
    assert!(hints.to_string().contains("2/3 complete"));
    assert!(!hints.to_string().contains("blocked by review"));
    // Unsaved open buffers must win over a disk rescan.
    lsp.notify(
        "workspace/didChangeWatchedFiles",
        json!({"changes":[{"uri":Url::from_file_path(&resource).unwrap(),"type":2}]}),
    );
    assert_eq!(lsp.diagnostics(2), json!([]));
    let hints = lsp.request(
        "textDocument/inlayHint",
        json!({"textDocument":{"uri":uri},"range":range}),
    );
    assert!(hints.to_string().contains("2/3 complete"));
    assert!(lsp.requests.contains(&"workspace/inlayHint/refresh".into()));
    assert!(
        lsp.requests
            .contains(&"workspace/semanticTokens/refresh".into())
    );
    lsp.request("shutdown", json!(null));
    lsp.notify("exit", json!(null));
}

#[test]
fn timers_apply_versioned_edits_refresh_without_typing_and_finish_countdowns() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("timers.jot");
    let uri = Url::from_file_path(&path).unwrap();
    let initial = "[watch] := stopwatch()\n[focus] := countdown(2s)\n- [ ] Work @timer(watch)\nSpent [watch.elapsed].\n";
    std::fs::write(&path, initial).unwrap();
    let mut lsp = Lsp::start(&root);
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"jot","version":1,"text":initial}}),
    );
    assert_eq!(lsp.diagnostics(1), json!([]));
    let range = json!({"start":{"line":0,"character":0},"end":{"line":99,"character":0}});
    let hints = lsp.request(
        "textDocument/inlayHint",
        json!({"textDocument":{"uri":uri},"range":range}),
    );
    assert!(hints.to_string().contains("elapsed · idle"));
    let completion = lsp.request(
        "textDocument/completion",
        json!({"textDocument":{"uri":uri},"position":{"line":3,"character":13}}),
    );
    assert!(
        completion
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["label"] == "elapsed")
    );
    assert!(
        !completion
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["label"] == "remaining")
    );
    let renamed = lsp.request("textDocument/rename", json!({"textDocument":{"uri":uri},"position":{"line":3,"character":16},"newName":"debugging"}));
    let edits: Vec<TextEdit> =
        serde_json::from_value(renamed["documentChanges"][0]["edits"].clone()).unwrap();
    assert!(
        jot::actions::apply_edits(initial, &edits)
            .unwrap()
            .contains("[debugging.elapsed]")
    );
    // Start from the checklist reference, not the timer declaration.
    let running = lsp.timer_action(&uri, initial, 2, 1, "Start timer 'watch'");
    assert!(running.contains("stopwatch(0s,"));
    // This request drains source-change refreshes; following requests are timer ticks.
    lsp.request(
        "textDocument/semanticTokens/full",
        json!({"textDocument":{"uri":uri}}),
    );
    lsp.wait_for_request("workspace/inlayHint/refresh");
    lsp.wait_for_request("workspace/inlayHint/refresh");
    let hints = lsp.request(
        "textDocument/inlayHint",
        json!({"textDocument":{"uri":uri},"range":range}),
    );
    assert!(hints.to_string().contains("elapsed · running"));
    let elapsed = hints
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["position"]["line"] == 3)
        .unwrap();
    assert_ne!(elapsed["label"], "0s");
    let paused = lsp.timer_action(&uri, &running, 0, 2, "Pause timer");
    let hints = lsp.request(
        "textDocument/inlayHint",
        json!({"textDocument":{"uri":uri},"range":range}),
    );
    assert!(hints.to_string().contains("elapsed · paused"));
    let resumed = lsp.timer_action(&uri, &paused, 0, 3, "Resume timer");
    let reset = lsp.timer_action(&uri, &resumed, 0, 4, "Reset timer");
    assert_eq!(reset, initial);
    let countdown = lsp.timer_action(&uri, &reset, 1, 5, "Start timer 'focus'");
    assert!(countdown.contains("countdown(2s, 0s,"));
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    loop {
        assert!(
            std::time::Instant::now() < deadline,
            "Countdown never finished"
        );
        lsp.wait_for_request("workspace/inlayHint/refresh");
        let hints = lsp.request(
            "textDocument/inlayHint",
            json!({"textDocument":{"uri":uri},"range":range}),
        );
        if hints.to_string().contains("00:00 remaining · done") {
            break;
        }
    }
    let hover = lsp.request(
        "textDocument/hover",
        json!({"textDocument":{"uri":uri},"position":{"line":3,"character":16}}),
    );
    assert!(hover.to_string().contains("watch.elapsed = 0s"));
    assert!(
        lsp.applied_edits.is_empty(),
        "Ticking must not edit the note"
    );
    // Drain the last refresh response, then an idle workspace should stay quiet.
    lsp.request(
        "textDocument/semanticTokens/full",
        json!({"textDocument":{"uri":uri}}),
    );
    let quiet_until = std::time::Instant::now() + Duration::from_millis(1400);
    loop {
        match lsp
            .output
            .recv_timeout(quiet_until.saturating_duration_since(std::time::Instant::now()))
        {
            Ok(message) => {
                assert_ne!(
                    message["method"], "workspace/inlayHint/refresh",
                    "Finished timer kept refreshing"
                );
                lsp.handle_server_request(&message);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
            Err(e) => panic!("Server stopped: {e}"),
        }
    }
    lsp.request("shutdown", json!(null));
    lsp.notify("exit", json!(null));
}
