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
    opened_documents: Vec<Value>,
}
impl Lsp {
    fn start(root: &std::path::Path) -> Self {
        Self::start_with_capabilities(
            root,
            json!({
                "textDocument": {"completion": {"completionItem": {"snippetSupport": true}}, "documentSymbol":{"hierarchicalDocumentSymbolSupport":true}},
                "window": {"showDocument": {"support": true}},
                "workspace": {
                    "inlayHint": {"refreshSupport": true},
                    "semanticTokens": {"refreshSupport": true},
                    "codeLens": {"refreshSupport": true},
                    "didChangeWatchedFiles": {"dynamicRegistration": true}
                }
            }),
        )
    }
    fn start_with_capabilities(root: &std::path::Path, capabilities: Value) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_wtf"))
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
            opened_documents: Vec::new(),
        };
        let result=client.request("initialize",json!({"processId":null,"rootUri":Url::from_directory_path(root).unwrap(),"capabilities":capabilities}));
        assert_eq!(result["capabilities"]["inlayHintProvider"], true);
        assert_eq!(
            result["capabilities"]["codeLensProvider"]["resolveProvider"],
            false
        );
        assert_eq!(result["capabilities"]["documentHighlightProvider"], true);
        assert!(
            result["capabilities"]["signatureHelpProvider"]["triggerCharacters"]
                .as_array()
                .unwrap()
                .contains(&json!(","))
        );
        assert_eq!(
            result["capabilities"]["renameProvider"]["prepareProvider"],
            true
        );
        assert_eq!(
            result["capabilities"]["semanticTokensProvider"]["legend"]["tokenTypes"],
            json!(wtf::highlighting::TOKEN_TYPES)
        );
        assert_eq!(
            result["capabilities"]["semanticTokensProvider"]["legend"]["tokenModifiers"],
            json!(wtf::highlighting::TOKEN_MODIFIERS)
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
            } else if method == "window/showDocument" {
                self.opened_documents.push(value["params"].clone());
                json!({"success": true})
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
        let text = wtf::actions::apply_edits(text, &edits).unwrap();
        self.notify("textDocument/didChange", json!({"textDocument":{"uri":uri,"version":version + 1},"contentChanges":[{"text":text}]}));
        assert_eq!(self.diagnostics(version as i32 + 1), json!([]));
        text
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        let response = self.request_raw(method, params);
        assert!(response.get("error").is_none(), "{response}");
        response["result"].clone()
    }
    fn request_raw(&mut self, method: &str, params: Value) -> Value {
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
                return v;
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

fn selected(text: &str, row: usize, needle: &str) -> Value {
    let line = text.lines().nth(row).unwrap();
    let start = line.find(needle).unwrap();
    serde_json::to_value(wtf::document::Span::new(row, start, start + needle.len()).range(text))
        .unwrap()
}

#[test]
fn raw_links_and_rich_tokens_work_over_lsp_and_follow_edits() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("links.wtf");
    let uri = Url::from_file_path(&path).unwrap();
    let source = "🦀 ./packing.wtf and https://example.com/docs.\n[focus] := countdown(25m)\n- [ ] Interview 09/17/2026 at 7AM\n";
    let mut lsp = Lsp::start(&root);
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"wtf","version":1,"text":source}}),
    );
    assert_eq!(lsp.diagnostics(1), json!([]));
    let links = lsp.request(
        "textDocument/documentLink",
        json!({"textDocument":{"uri":uri}}),
    );
    assert_eq!(links.as_array().unwrap().len(), 2);
    assert_eq!(
        links[0]["target"],
        json!(Url::from_file_path(root.join("packing.wtf")).unwrap())
    );
    let hover = lsp.request(
        "textDocument/hover",
        json!({"textDocument":{"uri":uri},"position":{"line":0,"character":4}}),
    );
    assert_eq!(hover["range"], links[0]["range"]);
    let data = lsp.request(
        "textDocument/semanticTokens/full",
        json!({"textDocument":{"uri":uri}}),
    );
    let expected =
        wtf::highlighting::semantic_tokens(&wtf::document::Document::parse(source.into()));
    assert_eq!(
        data,
        serde_json::to_value(lsp_types::SemanticTokens {
            result_id: None,
            data: expected
        })
        .unwrap()
    );
    let changed = "🦀 ../other.wtf\n[focus] := $3.30\n- [x] Interview 09/17/2026 at 7AM\n";
    lsp.notify(
        "textDocument/didChange",
        json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":changed}]}),
    );
    assert_eq!(lsp.diagnostics(2), json!([]));
    let links = lsp.request(
        "textDocument/documentLink",
        json!({"textDocument":{"uri":uri}}),
    );
    assert_eq!(links.as_array().unwrap().len(), 1);
    assert!(links[0]["target"].as_str().unwrap().ends_with("/other.wtf"));
    let data = lsp.request(
        "textDocument/semanticTokens/full",
        json!({"textDocument":{"uri":uri}}),
    );
    let expected =
        wtf::highlighting::semantic_tokens(&wtf::document::Document::parse(changed.into()));
    assert_eq!(
        data,
        serde_json::to_value(lsp_types::SemanticTokens {
            result_id: None,
            data: expected
        })
        .unwrap()
    );
}

#[test]
fn ignored_open_notes_keep_highlighting_without_workspace_token_invalidations() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join(".gitignore"), ".hidden/\n").unwrap();
    std::fs::create_dir(root.join(".hidden")).unwrap();
    let path = root.join(".hidden/TODOS.wtf");
    let uri = Url::from_file_path(&path).unwrap();
    let source = "# Notes\n\n## Checklist :launch\n\n- [ ] First\n- [X] Finished\n\ndone := completed(launch)\nleft := remaining(launch)\nwork := effort(launch)\n";
    std::fs::write(&path, source).unwrap();
    assert!(
        !wtf::workspace::Workspace::load(vec![root.clone()])
            .unwrap()
            .documents
            .contains_key(&path)
    );
    let mut lsp = Lsp::start(&root);
    let assert_highlighting = |lsp: &mut Lsp, text: &str| {
        // Wait until all change notifications have been sent, including refresh requests.
        lsp.wait_for_request("workspace/codeLens/refresh");
        let tokens = lsp.request(
            "textDocument/semanticTokens/full",
            json!({"textDocument":{"uri":uri}}),
        );
        let expected =
            wtf::highlighting::semantic_tokens(&wtf::document::Document::parse(text.into()));
        assert!(!expected.is_empty());
        assert_eq!(
            tokens["data"],
            serde_json::to_value(lsp_types::SemanticTokens {
                result_id: None,
                data: expected
            })
            .unwrap()["data"]
        );
        assert!(
            !lsp.requests
                .iter()
                .any(|r| r == "workspace/semanticTokens/refresh"),
            "Document edits must not invalidate highlighting across the workspace"
        );
    };
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"wtf","version":1,"text":source}}),
    );
    assert_highlighting(&mut lsp, source);
    for (i, title) in ["First 🦀", "First 🦀 edit", "First", "First again"]
        .into_iter()
        .enumerate()
    {
        let text = source.replace("First", title);
        lsp.notify(
            "textDocument/didChange",
            json!({"textDocument":{"uri":uri,"version":i + 2},"contentChanges":[{"text":text}]}),
        );
        assert_highlighting(&mut lsp, &text);
        // A watcher rescan must retain an ignored note's unsaved buffer and its tokens.
        lsp.notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes":[{"uri":uri,"type":2}]}),
        );
        assert_highlighting(&mut lsp, &text);
        std::fs::write(&path, &text).unwrap();
        lsp.notify("textDocument/didSave", json!({"textDocument":{"uri":uri}}));
        assert_highlighting(&mut lsp, &text);
    }
    // Changing a second note must not clear the ignored note's highlighting either.
    let other = Url::from_file_path(root.join("other.wtf")).unwrap();
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":other,"languageId":"wtf","version":1,"text":"1:value\n"}}),
    );
    assert_highlighting(&mut lsp, &source.replace("First", "First again"));
    lsp.notify(
        "textDocument/didClose",
        json!({"textDocument":{"uri":other}}),
    );
    assert_highlighting(&mut lsp, &source.replace("First", "First again"));
}

#[test]
fn document_symbols_use_shared_hierarchy_and_follow_unsaved_edits() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("symbols.wtf");
    let uri = Url::from_file_path(&path).unwrap();
    let text = "# Trip\n[$3,000]:budget\n## Money\n[remaining] := budget - $2,444\n- [ ] Pack :pack\n  - [x] Passport\n[focus] := countdown(25m)\n";
    std::fs::write(&path, text).unwrap();
    let mut lsp = Lsp::start(&root);
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"wtf","version":1,"text":text}}),
    );
    assert_eq!(lsp.diagnostics(1), json!([]));
    let symbols = lsp.request(
        "textDocument/documentSymbol",
        json!({"textDocument":{"uri":uri}}),
    );
    let ws = wtf::workspace::Workspace {
        roots: vec![root],
        documents: [(path.clone(), wtf::document::Document::parse(text.into()))].into(),
        cache: Default::default(),
        lookups: Default::default(),
        modules: Default::default(),
    };
    let shared = wtf::symbols::document_symbols(&ws, &path, chrono::Local::now().fixed_offset());
    assert_eq!(symbols, serde_json::to_value(shared).unwrap());
    assert_eq!(
        symbols[0]["children"][1]["children"][0]["detail"],
        "Money · $556"
    );
    let changed = text
        .replace("$2,444", "$1,410")
        .replace("## Money", "## Cash");
    lsp.notify(
        "textDocument/didChange",
        json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":changed}]}),
    );
    assert_eq!(lsp.diagnostics(2), json!([]));
    let symbols = lsp.request(
        "textDocument/documentSymbol",
        json!({"textDocument":{"uri":uri}}),
    );
    assert_eq!(symbols[0]["children"][1]["name"], "Cash");
    assert_eq!(
        symbols[0]["children"][1]["children"][0]["detail"],
        "Money · $1,590"
    );
    assert!(lsp.applied_edits.is_empty());
    assert_eq!(std::fs::read_to_string(path).unwrap(), text);
}

#[test]
fn document_symbols_fall_back_to_flat_locations_for_older_clients() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("symbols.wtf");
    let uri = Url::from_file_path(&path).unwrap();
    let text = "# Trip\n[42]:answer\n## Packing\n- [ ] Passport\n";
    std::fs::write(&path, text).unwrap();
    let mut lsp = Lsp::start_with_capabilities(&root, json!({}));
    let symbols = lsp.request(
        "textDocument/documentSymbol",
        json!({"textDocument":{"uri":uri}}),
    );
    assert_eq!(symbols.as_array().unwrap().len(), 4);
    assert_eq!(symbols[1]["name"], "answer");
    assert_eq!(symbols[1]["containerName"], "Trip");
    assert_eq!(symbols[1]["location"]["uri"], uri.as_str());
    assert_eq!(symbols[1]["location"]["range"], selected(text, 1, "answer"));
    assert_eq!(symbols[3]["containerName"], "Packing");
    assert!(symbols[0]["children"].is_null());
}

#[test]
fn tables_and_column_interactions_work_over_standard_lsp() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let data_path = root.join("data.wtf");
    let calc_path = root.join("calc.wtf");
    let data_uri = Url::from_file_path(&data_path).unwrap();
    let calc_uri = Url::from_file_path(&calc_path).unwrap();
    let data = "# Groceries\n[groceries] := table\n|item|quantity|price|\n|---|---|---|\n|apple|2|$3.30|\n|pear|4|$4.30|\n";
    let calc = "[total] := sum(groceries, quantity * price)\nCost [total].\n";
    std::fs::write(&data_path, data).unwrap();
    std::fs::write(&calc_path, calc).unwrap();
    let mut lsp = Lsp::start(&root);
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":data_uri,"languageId":"wtf","version":1,"text":data}}),
    );
    assert_eq!(lsp.diagnostics(1), json!([]));
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":calc_uri,"languageId":"wtf","version":1,"text":calc}}),
    );
    assert_eq!(lsp.diagnostics(1), json!([]));
    let at = json!({"textDocument":{"uri":calc_uri},"position":selected(calc,0,"price")["start"]});
    let completion = lsp.request("textDocument/completion", at.clone());
    assert!(
        completion
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["label"] == "quantity")
    );
    let signature = lsp.request("textDocument/signatureHelp", at.clone());
    assert!(
        signature["signatures"][0]["label"]
            .as_str()
            .unwrap()
            .starts_with("sum(")
    );
    let definition = lsp.request("textDocument/definition", at.clone());
    assert_eq!(definition["uri"], data_uri.as_str());
    assert_eq!(definition["range"], selected(data, 2, "price"));
    let hover = lsp.request("textDocument/hover", at.clone());
    assert!(hover.to_string().contains("Column of `groceries`"));
    let references = lsp.request("textDocument/references",json!({"textDocument":{"uri":data_uri},"position":selected(data,2,"price")["start"],"context":{"includeDeclaration":true}}));
    assert_eq!(references.as_array().unwrap().len(), 2);
    let edit = lsp.request(
        "textDocument/rename",
        json!({"textDocument":{"uri":calc_uri},"position":at["position"],"newName":"unit_price"}),
    );
    assert_eq!(edit["documentChanges"].as_array().unwrap().len(), 2);
    assert!(
        edit["documentChanges"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["textDocument"]["version"] == 1)
    );
    let collision = lsp.request_raw(
        "textDocument/rename",
        json!({"textDocument":{"uri":calc_uri},"position":at["position"],"newName":"quantity"}),
    );
    assert!(collision.get("error").is_some());
    let formatted = lsp.request(
        "textDocument/formatting",
        json!({"textDocument":{"uri":data_uri},"options":{"tabSize":2,"insertSpaces":true}}),
    );
    let edits: Vec<TextEdit> = serde_json::from_value(formatted).unwrap();
    let formatted = wtf::actions::apply_edits(data, &edits).unwrap();
    assert!(formatted.contains("| item  | quantity | price |"));
    let symbols = lsp.request(
        "textDocument/documentSymbol",
        json!({"textDocument":{"uri":data_uri}}),
    );
    assert_eq!(symbols[0]["children"][0]["name"], "groceries");
    assert_eq!(symbols[0]["children"][0]["children"][2]["name"], "price");
    let range = json!({"start":{"line":0,"character":0},"end":{"line":20,"character":0}});
    let hints = lsp.request(
        "textDocument/inlayHint",
        json!({"textDocument":{"uri":calc_uri},"range":range}),
    );
    assert_eq!(hints[0]["label"], "= $23.80");
    assert_eq!(hints[1]["label"], "$23.80");
    assert!(
        hints[0]["tooltip"]["value"]
            .as_str()
            .unwrap()
            .contains("Row 2: $17.20")
    );
    let changed = data.replace("|pear|4|", "|pear|5|");
    lsp.notify(
        "textDocument/didChange",
        json!({"textDocument":{"uri":data_uri,"version":2},"contentChanges":[{"text":changed}]}),
    );
    let hints = lsp.request(
        "textDocument/inlayHint",
        json!({"textDocument":{"uri":calc_uri},"range":range}),
    );
    assert_eq!(hints[0]["label"], "= $28.10");
    assert!(lsp.applied_edits.is_empty());
    assert_eq!(std::fs::read_to_string(data_path).unwrap(), data);
}

#[test]
fn prose_value_inlays_follow_calculations_and_update_after_edits() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("trip.wtf");
    let uri = Url::from_file_path(&path).unwrap();
    let text = "# Trip\n\nOur budget is [$3,000]:budget.\n\nWe've spent [$2,444]:spent.\n\n[remaining] := budget - spent\n\nWe have [remaining] remaining.\n";
    std::fs::write(&path, text).unwrap();
    let mut lsp = Lsp::start(&root);
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"wtf","version":1,"text":text}}),
    );
    assert_eq!(lsp.diagnostics(1), json!([]));
    let range = json!({"start":{"line":0,"character":0},"end":{"line":9,"character":0}});
    let hints = lsp.request(
        "textDocument/inlayHint",
        json!({"textDocument":{"uri":uri},"range":range}),
    );
    assert_eq!(hints.as_array().unwrap().len(), 2, "{hints}");
    assert_eq!(hints[0]["label"], "= $556");
    assert_eq!(
        hints[1]["position"],
        selected(text, 8, "[remaining]")["end"]
    );
    assert_eq!(hints[1]["label"], "$556");
    assert_eq!(hints[1]["paddingLeft"], true);
    assert!(hints[1]["textEdits"].is_null());
    assert!(
        hints[1]["tooltip"]["value"]
            .as_str()
            .unwrap()
            .contains("$3,000 - $2,444")
    );

    let changed = text.replace("$2,444", "$1,410");
    lsp.notify(
        "textDocument/didChange",
        json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":changed}]}),
    );
    assert_eq!(lsp.diagnostics(2), json!([]));
    let hints = lsp.request(
        "textDocument/inlayHint",
        json!({"textDocument":{"uri":uri},"range":range}),
    );
    assert_eq!(hints[0]["label"], "= $1,590");
    assert_eq!(hints[1]["label"], "$1,590");
    assert!(lsp.applied_edits.is_empty());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
}

#[test]
fn rich_editor_interactions_work_over_stdio() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("interactions.wtf");
    let uri = Url::from_file_path(&path).unwrap();
    let text = "[$3,000]:budget\n[$1,410]:spent\n[cash] := budget - spent\nUse [cash].\n[focus] := countdown(25m, 0s)\n- [ ] Review :review @timer(focus)\nPlain prose.\n";
    std::fs::write(&path, text).unwrap();
    let mut lsp = Lsp::start(&root);
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"wtf","version":1,"text":text}}),
    );
    assert_eq!(lsp.diagnostics(1), json!([]));
    let symbol = json!({"textDocument":{"uri":uri},"position":selected(text, 3, "cash")["start"]});
    let hover = lsp.request("textDocument/hover", symbol.clone());
    assert!(hover.to_string().contains("$3,000 - $1,410"));
    assert!(hover.to_string().contains("#L1"));
    let highlights = lsp.request("textDocument/documentHighlight", symbol.clone());
    assert_eq!(highlights.as_array().unwrap().len(), 2);
    assert!(
        highlights
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["kind"] == 3)
    ); // declaration/write
    assert!(
        highlights
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["kind"] == 2)
    ); // reference/read
    let prepared = lsp.request("textDocument/prepareRename", symbol);
    assert_eq!(prepared["placeholder"], "cash");
    assert_eq!(prepared["range"], selected(text, 3, "cash"));
    let completion = lsp.request(
        "textDocument/completion",
        json!({"textDocument":{"uri":uri},"position":selected(text, 5, "@timer(")["end"]}),
    );
    assert_eq!(completion.as_array().unwrap().len(), 1);
    assert_eq!(completion[0]["label"], "focus");
    assert!(
        completion[0]["detail"]
            .as_str()
            .unwrap()
            .contains("Countdown")
    );
    let signature = lsp.request(
        "textDocument/signatureHelp",
        json!({"textDocument":{"uri":uri},"position":selected(text, 4, "25m, ")["end"]}),
    );
    assert_eq!(signature["activeParameter"], 1);
    assert!(
        signature["signatures"][0]["label"]
            .as_str()
            .unwrap()
            .contains("elapsed?: Duration")
    );
    let range = json!({"start":{"line":6,"character":0},"end":{"line":6,"character":0}});
    let actions = lsp.request(
        "textDocument/codeAction",
        json!({"textDocument":{"uri":uri},"range":range,"context":{"diagnostics":[]}}),
    );
    assert_eq!(actions, json!([]));
    let actions = lsp.request("textDocument/codeAction", json!({"textDocument":{"uri":uri},"range":selected(text, 2, "budget - spent"),"context":{"diagnostics":[],"only":["refactor.extract"]}}));
    let edit = &actions[0]["edit"]["documentChanges"][0];
    assert_eq!(edit["textDocument"]["version"], 1);
    let edits: Vec<TextEdit> = serde_json::from_value(edit["edits"].clone()).unwrap();
    let extracted = wtf::actions::apply_edits(text, &edits).unwrap();
    assert!(extracted.contains("[calculation] := budget - spent\n[cash] := calculation"));
    lsp.notify(
        "textDocument/didChange",
        json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":extracted}]}),
    );
    assert_eq!(lsp.diagnostics(2), json!([]));
    let typo = extracted.replace("[cash].", "[csah].");
    lsp.notify(
        "textDocument/didChange",
        json!({"textDocument":{"uri":uri,"version":3},"contentChanges":[{"text":typo}]}),
    );
    let ds = lsp.diagnostics(3);
    assert_eq!(ds.as_array().unwrap().len(), 1);
    assert_eq!(ds[0]["code"], "unknown-name");
    assert_eq!(ds[0]["range"], selected(&typo, 4, "csah"));
    let actions = lsp.request("textDocument/codeAction", json!({"textDocument":{"uri":uri},"range":ds[0]["range"],"context":{"diagnostics":ds,"only":["quickfix"]}}));
    assert!(
        actions
            .as_array()
            .unwrap()
            .iter()
            .all(|a| a["kind"] == "quickfix")
    );
    let fix = actions
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["title"] == "Change 'csah' to 'cash'")
        .unwrap();
    let edits: Vec<TextEdit> =
        serde_json::from_value(fix["edit"]["documentChanges"][0]["edits"].clone()).unwrap();
    let fixed = wtf::actions::apply_edits(&typo, &edits).unwrap();
    assert_eq!(fixed, extracted);
    lsp.notify(
        "textDocument/didChange",
        json!({"textDocument":{"uri":uri,"version":4},"contentChanges":[{"text":fixed}]}),
    );
    assert_eq!(lsp.diagnostics(4), json!([]));
}

#[test]
fn clickable_controls_apply_versioned_edits_open_resources_and_reject_stale_targets() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("controls.wtf");
    let uri = Url::from_file_path(&path).unwrap();
    let text = "- [ ] First :first\n[./receipt.png]:receipt\nUse [receipt].\n";
    std::fs::write(&path, text).unwrap();
    let mut lsp = Lsp::start(&root);
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"wtf","version":1,"text":text}}),
    );
    assert_eq!(lsp.diagnostics(1), json!([]));
    let lenses = lsp.request("textDocument/codeLens", json!({"textDocument":{"uri":uri}}));
    let commands: Vec<_> = lenses
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["command"].clone())
        .collect();
    let open = commands
        .iter()
        .find(|c| c["title"] == "Open image")
        .unwrap();
    lsp.request("workspace/executeCommand", open.clone());
    assert_eq!(lsp.opened_documents.len(), 1);
    assert_eq!(
        lsp.opened_documents[0]["uri"],
        Url::from_file_path(root.join("receipt.png"))
            .unwrap()
            .as_str()
    );
    assert_eq!(lsp.opened_documents[0]["external"], false);
    let complete = commands
        .iter()
        .find(|c| c["title"] == "Complete task")
        .unwrap();
    lsp.request("workspace/executeCommand", complete.clone());
    let edit = lsp.applied_edits.pop().unwrap();
    assert_eq!(edit["documentChanges"][0]["textDocument"]["version"], 1);
    let edits: Vec<TextEdit> =
        serde_json::from_value(edit["documentChanges"][0]["edits"].clone()).unwrap();
    let completed = wtf::actions::apply_edits(text, &edits).unwrap();
    assert!(completed.starts_with("- [x] First"));
    lsp.notify(
        "textDocument/didChange",
        json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":completed}]}),
    );
    assert_eq!(lsp.diagnostics(2), json!([]));
    let lenses = lsp.request("textDocument/codeLens", json!({"textDocument":{"uri":uri}}));
    let reopen = lenses
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["command"]["title"] == "Reopen task")
        .unwrap()["command"]
        .clone();
    let moved = format!("- [ ] Different task\n{completed}");
    lsp.notify(
        "textDocument/didChange",
        json!({"textDocument":{"uri":uri,"version":3},"contentChanges":[{"text":moved}]}),
    );
    assert_eq!(lsp.diagnostics(3), json!([]));
    let rejected = lsp.request_raw("workspace/executeCommand", reopen);
    assert!(
        rejected["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Source changed")
    );
    assert!(
        lsp.applied_edits.is_empty(),
        "Stale controls must not modify a different task"
    );
    assert!(
        !root.join(".wtf").exists(),
        "Opening resources must not trigger a GitHub fetch/cache write"
    );
}

#[test]
fn live_diagnostics_and_codelenses_refresh_without_source_edits() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("live.wtf");
    let uri = Url::from_file_path(&path).unwrap();
    let start = (chrono::Local::now() + chrono::Duration::seconds(1)).to_rfc3339();
    let text = format!("[focus] := countdown(3s, 0s, {start})\n[rate] := 1s / focus.elapsed\n");
    std::fs::write(&path, &text).unwrap();
    let mut lsp = Lsp::start(&root);
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"wtf","version":1,"text":text}}),
    );
    assert!(lsp.diagnostics(1).to_string().contains("Division by zero"));
    // A clock tick, not didChange, clears the diagnostic at the same document version.
    assert_eq!(lsp.diagnostics(1), json!([]));
    let previous = lsp
        .requests
        .iter()
        .filter(|s| *s == "workspace/codeLens/refresh")
        .count();
    lsp.wait_for_request("workspace/codeLens/refresh");
    assert_eq!(
        lsp.requests
            .iter()
            .filter(|s| *s == "workspace/codeLens/refresh")
            .count(),
        previous + 1
    );
    let lenses = lsp.request("textDocument/codeLens", json!({"textDocument":{"uri":uri}}));
    assert!(!lenses.to_string().contains("Pause timer"));
    assert!(lenses.to_string().contains("Reset timer"));
    assert!(lsp.applied_edits.is_empty());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
}

#[test]
fn clients_without_snippet_or_refresh_support_receive_plain_completions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("plain.wtf");
    let uri = Url::from_file_path(&path).unwrap();
    let text = "[value] := cou\n";
    std::fs::write(&path, text).unwrap();
    let mut lsp = Lsp::start_with_capabilities(&root, json!({}));
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"wtf","version":1,"text":text}}),
    );
    lsp.diagnostics(1);
    let items = lsp.request(
        "textDocument/completion",
        json!({"textDocument":{"uri":uri},"position":selected(text, 0, "cou")["end"]}),
    );
    let item = items
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["label"] == "countdown(25m)")
        .unwrap();
    assert_eq!(item["textEdit"]["newText"], "countdown(25m)");
    assert_ne!(item["insertTextFormat"], 2);
    assert!(!lsp.requests.iter().any(|m| m.ends_with("/refresh")));
}

#[test]
fn zed_workflow_updates_hints_highlights_links_tasks_and_cross_file_names() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let resource = root.join("resources.wtf");
    std::fs::write(
        &resource,
        "[./receipt.png]:receipt\n[2026-09-25]:departure\n",
    )
    .unwrap();
    let path = root.join("daily.wtf");
    let uri = Url::from_file_path(&path).unwrap();
    let text = "# Release :release\n- [x] Parser\n- [ ] Review [receipt] :review @estimate(30m)\n- [ ] Publish @after(review) @due(departure-7d)\n[progress] := completed(release)/total(release)\n";
    std::fs::write(&path, text).unwrap();
    let mut lsp = Lsp::start(&root);
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"wtf","version":1,"text":text}}),
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
    let changed = wtf::actions::apply_edits(text, &edits).unwrap();
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
        !lsp.requests
            .contains(&"workspace/semanticTokens/refresh".into())
    );
    lsp.request("shutdown", json!(null));
    lsp.notify("exit", json!(null));
}

#[test]
fn timers_apply_versioned_edits_refresh_without_typing_and_finish_countdowns() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("timers.wtf");
    let uri = Url::from_file_path(&path).unwrap();
    let initial = "[watch] := stopwatch()\n[focus] := countdown(2s)\n- [ ] Work @timer(watch)\nSpent [watch.elapsed].\n";
    std::fs::write(&path, initial).unwrap();
    let mut lsp = Lsp::start(&root);
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"wtf","version":1,"text":initial}}),
    );
    assert_eq!(lsp.diagnostics(1), json!([]));
    let range = json!({"start":{"line":0,"character":0},"end":{"line":99,"character":0}});
    let hints = lsp.request(
        "textDocument/inlayHint",
        json!({"textDocument":{"uri":uri},"range":range}),
    );
    assert!(hints.to_string().contains("elapsed · ○ idle"));
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
        wtf::actions::apply_edits(initial, &edits)
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
    assert!(hints.to_string().contains("elapsed · ▸ running"));
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
    assert!(hints.to_string().contains("elapsed · ‖ paused"));
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
        if hints.to_string().contains("00:00 remaining · ✓ done") {
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

#[test]
fn on_type_formatting_and_call_hierarchy_expose_the_dependency_graph() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("plan.wtf");
    let uri = Url::from_file_path(&path).unwrap();
    let text = "[$3,000]:budget\n[$1,410]:spent\n[cash] := budget - spent\n[half] := cash / 2\n[t] := table\n| item | qty |\n|---|---|\n| apple | 2 |\n# Plan :plan\n- [ ] Buy :buy @after(cash > spent)\n\n";
    std::fs::write(&path, text).unwrap();
    let mut lsp = Lsp::start(&root);
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"wtf","version":1,"text":text}}),
    );
    assert_eq!(lsp.diagnostics(1), json!([]));

    // Enter at the end of the task line continues the checklist.
    let edits = lsp.request(
        "textDocument/onTypeFormatting",
        json!({"textDocument":{"uri":uri},"position":{"line":10,"character":0},"ch":"\n","options":{"tabSize":2,"insertSpaces":true}}),
    );
    let edits: Vec<TextEdit> = serde_json::from_value(edits).unwrap();
    let continued = wtf::actions::apply_edits(text, &edits).unwrap();
    assert!(
        continued.ends_with("@after(cash > spent)\n- [ ] \n"),
        "{continued}"
    );

    // A closing pipe on the last row aligns the whole table.
    let edits = lsp.request(
        "textDocument/onTypeFormatting",
        json!({"textDocument":{"uri":uri},"position":{"line":7,"character":13},"ch":"|","options":{"tabSize":2,"insertSpaces":true}}),
    );
    let edits: Vec<TextEdit> = serde_json::from_value(edits).unwrap();
    let aligned = wtf::actions::apply_edits(text, &edits).unwrap();
    assert!(
        aligned.contains("| item  | qty |\n| ----- | --- |\n| apple | 2   |\n"),
        "{aligned}"
    );

    // Call hierarchy on cash: budget and spent feed it; half and Buy read it.
    let items = lsp.request(
        "textDocument/prepareCallHierarchy",
        json!({"textDocument":{"uri":uri},"position":selected(text,2,"cash")["start"]}),
    );
    assert_eq!(items[0]["name"], "cash");
    assert_eq!(items[0]["detail"], "Money · $1,590");
    assert_eq!(items[0]["selectionRange"], selected(text, 2, "cash"));
    let outgoing = lsp.request("callHierarchy/outgoingCalls", json!({"item":items[0]}));
    let mut reads: Vec<_> = outgoing
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["to"]["name"].as_str().unwrap().to_string())
        .collect();
    reads.sort();
    assert_eq!(reads, ["budget", "spent"]);
    assert_eq!(outgoing[0]["fromRanges"][0], selected(text, 2, "budget"));
    let incoming = lsp.request("callHierarchy/incomingCalls", json!({"item":items[0]}));
    let mut readers: Vec<_> = incoming
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            format!(
                "{} · {}",
                c["from"]["name"].as_str().unwrap(),
                c["from"]["detail"].as_str().unwrap()
            )
        })
        .collect();
    readers.sort();
    assert_eq!(readers, ["buy · task · incomplete", "half · Money · $795"]);
    // Items carry opaque data; a stale item after an edit is rejected, not a crash.
    lsp.notify(
        "textDocument/didChange",
        json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":"Nothing here\n"}]}),
    );
    assert_eq!(lsp.diagnostics(2), json!([]));
    let stale = lsp.request("callHierarchy/incomingCalls", json!({"item":items[0]}));
    assert!(stale.is_null(), "{stale}");
}

#[test]
fn plans_solve_over_lsp_and_variable_renames_touch_each_occurrence_once() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("bakery.wtf");
    let uri = Url::from_file_path(&path).unwrap();
    let text = "[bakery] := maximize($3 * bagels + $1.25 * doughnuts)\n| constraint | expression |\n| ---------- | ---------- |\n| flour | 12 * bagels + 6.5 * doughnuts <= 400 |\n| bagel_min | bagels >= 12 |\n| doughnut_min | doughnuts >= 14 |\nBake [bakery.bagels] bagels.\n";
    std::fs::write(&path, text).unwrap();
    let mut lsp = Lsp::start(&root);
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"wtf","version":1,"text":text}}),
    );
    assert_eq!(lsp.diagnostics(1), json!([]));
    let range = json!({"start":{"line":0,"character":0},"end":{"line":20,"character":0}});
    let hints = lsp.request(
        "textDocument/inlayHint",
        json!({"textDocument":{"uri":uri},"range":range}),
    );
    assert_eq!(hints[0]["label"], "= $94.75 · bagels 25.75 · doughnuts 14");
    assert!(hints.to_string().contains("400 ≤ 400 · ● binding"));
    let at = json!({"textDocument":{"uri":uri},"position":selected(text,3,"bagels")["start"]});
    let hover = lsp.request("textDocument/hover", at.clone());
    assert!(
        hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("Decision variable of")
    );
    let edit = lsp.request(
        "textDocument/rename",
        json!({"textDocument":{"uri":uri},"position":at["position"],"newName":"rolls"}),
    );
    let edits: Vec<TextEdit> =
        serde_json::from_value(edit["documentChanges"][0]["edits"].clone()).unwrap();
    // Objective, two constraint rows, one property access: four distinct ranges.
    assert_eq!(edits.len(), 4, "{edits:?}");
    let renamed = wtf::actions::apply_edits(text, &edits).unwrap();
    assert_eq!(
        renamed.matches("bagels").count(),
        1,
        "only prose stays: {renamed}"
    );
    assert!(
        renamed.contains("$3 * rolls") && renamed.contains("[bakery.rolls] bagels"),
        "{renamed}"
    );
    // Breaking the plan reports on the objective, not on every variable.
    let broken = text.replace(
        "| bagel_min | bagels >= 12 |",
        "| bagel_min | bagels >= 40 |",
    );
    lsp.notify(
        "textDocument/didChange",
        json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":broken}]}),
    );
    let diagnostics = lsp.diagnostics(2);
    assert_eq!(diagnostics.as_array().unwrap().len(), 1, "{diagnostics}");
    assert!(
        diagnostics[0]["message"]
            .as_str()
            .unwrap()
            .starts_with("No values satisfy")
    );
}

#[test]
fn editor_agenda_uses_the_replaceable_standard_library() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    std::fs::write(root.join("tasks.wtf"), "- [ ] Active\n- [x] Finished\n").unwrap();
    let mut lsp = Lsp::start(&root);
    let command = json!({"command":"wtf.today","arguments":[]});
    lsp.request("workspace/executeCommand", command.clone());
    let view = root.join(".wtf/today.md");
    let original = std::fs::read_to_string(&view).unwrap();
    assert!(original.contains("— Active"), "{original}");
    assert!(!original.contains("— Finished"), "{original}");
    assert_eq!(
        lsp.opened_documents.last().unwrap()["uri"],
        Url::from_file_path(&view).unwrap().as_str()
    );

    std::fs::create_dir_all(root.join(".wtf/modules")).unwrap();
    std::fs::write(
        root.join(".wtf/modules/agenda.wtf"),
        r#"module := {api: 1, id: "agenda", kind: "library", inputs: []}
between := fn(entries, first, last) => filter(entries, fn(e) => e.done)
"#,
    )
    .unwrap();
    // The command rescans saved modules before evaluating the same import as a query.
    lsp.request("workspace/executeCommand", command);
    let replaced = std::fs::read_to_string(view).unwrap();
    assert!(replaced.contains("— Finished"), "{replaced}");
    assert!(!replaced.contains("— Active"), "{replaced}");
    assert_eq!(
        lsp.request("wtf/query", json!({"query":"map(import(\"agenda\").between(entries, today(), today()), fn(e) => e.title)"}))["rows"],
        json!(["Finished"])
    );
}

#[test]
fn workspace_queries_read_live_buffers_and_return_typed_versioned_snapshots() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("n.wtf");
    let uri = Url::from_file_path(&path).unwrap();
    let disk = "$5:price\n- [ ] Saved\n";
    std::fs::write(&path, disk).unwrap();
    let mut lsp = Lsp::start(&root);
    lsp.notify("textDocument/didOpen",json!({"textDocument":{"uri":uri,"languageId":"wtf","version":1,"text":"$9:price\n- [ ] Unsaved\n"}}));
    assert_eq!(lsp.diagnostics(1), json!([]));
    let now = "2026-09-16T23:30:00-04:00";
    let result = lsp.request(
        "wtf/query",
        json!({"query":"values | select {name, value, day:today(), clock:now()}","now":now}),
    );
    assert_eq!(result["schemaVersion"], 1);
    assert_eq!(result["versions"][uri.as_str()], 1);
    assert_eq!(
        result["rows"][0]["value"],
        json!({"type":"money","amount":9.0,"currency":"USD"})
    );
    assert_eq!(result["rows"][0]["day"]["value"], "2026-09-16");
    assert_eq!(result["rows"][0]["clock"]["value"], now);
    assert_eq!(
        lsp.request(
            "wtf/query",
            json!({"query":"tasks | where leaf && !done | sort source.path, source.line | select title","now":now})
        )["rows"],
        json!(["Unsaved"])
    );
    lsp.notify("textDocument/didChange",json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":"$12:price\n- [x] Finished\n"}]}));
    assert_eq!(lsp.diagnostics(2), json!([]));
    let changed = lsp.request(
        "wtf/query",
        json!({"query":"tasks | where leaf && !done | sort source.path, source.line","now":now}),
    );
    assert_eq!(changed["versions"][uri.as_str()], 2);
    assert_eq!(changed["rows"], json!([]));
    let invalid = lsp.request_raw("wtf/query", json!({"query":"tasks | where ("}));
    assert_eq!(invalid["error"]["code"], -32602);
    assert_eq!(std::fs::read_to_string(path).unwrap(), disk);
}

#[test]
fn native_actions_reject_the_same_malformed_commands_as_the_shared_codec() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("actions.wtf");
    std::fs::write(&path, "- [ ] Task\n").unwrap();
    let uri = Url::from_file_path(&path).unwrap();
    let mut lsp = Lsp::start(&root);
    for (name, args) in [
        ("wtf.task", json!([uri, 0, "- [ ] Task", "extra"])),
        ("wtf.timer", json!([uri, "focus", "explode"])),
        (
            "wtf.openResource",
            json!([uri, -1, "line", "https://example.com"]),
        ),
        ("wtf.refresh", json!([uri, "extra"])),
        ("unknown", json!([])),
    ] {
        let expected = wtf::commands::Action::decode(name, args.as_array().unwrap()).unwrap_err();
        let response = lsp.request_raw(
            "workspace/executeCommand",
            json!({"command":name,"arguments":args}),
        );
        assert_eq!(response["error"]["code"], -32602);
        assert_eq!(response["error"]["message"], expected);
    }
    assert!(lsp.applied_edits.is_empty());
    assert!(lsp.opened_documents.is_empty());
    assert!(!root.join(".wtf").exists());
}

#[test]
fn module_and_standard_library_buffers_highlight_without_activating_unsaved_source() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let module = root.join(".wtf/modules/docs.wtf");
    let library = root.join("stdlib/format.wtf");
    for path in [&module, &library] {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    }
    let source = "module := {api: 1, id: \"docs\", kind: \"link\", hosts: [\"docs.example\"]}\ninlay := fn(ctx) => \"saved\"\n";
    let standard = include_str!("../stdlib/format.wtf");
    std::fs::write(&module, source).unwrap();
    std::fs::write(&library, standard).unwrap();
    let note = root.join("main.wtf");
    std::fs::write(&note, "https://docs.example/start\n").unwrap();
    let mut client = Lsp::start(&root);
    let hints = json!({"textDocument":{"uri":Url::from_file_path(&note).unwrap()},"range":{"start":{"line":0,"character":0},"end":{"line":1,"character":0}}});
    let assert_tokens = |client: &mut Lsp, uri: &Url, text: &str| {
        let expected =
            wtf::highlighting::semantic_tokens(&wtf::document::Document::parse(text.into()));
        assert!(!expected.is_empty());
        assert_eq!(
            client.request(
                "textDocument/semanticTokens/full",
                json!({"textDocument":{"uri":uri}})
            ),
            serde_json::to_value(lsp_types::SemanticTokens {
                result_id: None,
                data: expected
            })
            .unwrap()
        );
    };
    for (path, text) in [(&module, source), (&library, standard)] {
        let uri = Url::from_file_path(path).unwrap();
        client.notify(
            "textDocument/didOpen",
            json!({"textDocument":{"uri":uri,"languageId":"wtf","version":1,"text":text}}),
        );
        client.wait_for_request("workspace/codeLens/refresh");
        assert_tokens(&mut client, &uri, text);
        let changed = format!(
            "{}\nunsaved := fn(value) => value + 1\n",
            text.replace("saved", "unsaved")
        );
        client.notify(
            "textDocument/didChange",
            json!({"textDocument":{"uri":uri,"version":3},"contentChanges":[{"text":changed}]}),
        );
        client.wait_for_request("workspace/codeLens/refresh");
        assert_tokens(&mut client, &uri, &changed);
        // A stale notification cannot replace the newer editor buffer.
        client.notify("textDocument/didChange", json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":"old buffer"}]}));
        assert_tokens(&mut client, &uri, &changed);
        // Query rescans preserve editor buffers and exclude module declarations.
        assert_eq!(
            client.request("wtf/query", json!({"query":"values"}))["rows"],
            json!([])
        );
        assert_tokens(&mut client, &uri, &changed);
        assert_eq!(
            client.request("textDocument/inlayHint", hints.clone())[0]["label"],
            "saved"
        );
        client.notify("textDocument/didClose", json!({"textDocument":{"uri":uri}}));
        client.wait_for_request("workspace/codeLens/refresh");
        assert!(
            client
                .request(
                    "textDocument/semanticTokens/full",
                    json!({"textDocument":{"uri":uri}})
                )
                .is_null()
        );
    }
    // Saving still activates the disk version through the normal reload path.
    std::fs::write(&module, source.replace("saved", "reloaded")).unwrap();
    client.notify(
        "textDocument/didSave",
        json!({"textDocument":{"uri":Url::from_file_path(&module).unwrap()}}),
    );
    client.wait_for_request("workspace/codeLens/refresh");
    assert_eq!(
        client.request("textDocument/inlayHint", hints)[0]["label"],
        "reloaded"
    );
}

#[test]
fn functional_modules_hot_reload_over_lsp_and_keep_last_good_version() {
    let dir = tempfile::tempdir().unwrap();
    let module_dir = dir.path().join(".wtf/modules");
    std::fs::create_dir_all(&module_dir).unwrap();
    let module = module_dir.join("docs.wtf");
    let source = "module := {api: 1, id: \"docs\", kind: \"link\", hosts: [\"docs.example\"]}\ninlay := fn(ctx) => \"first\"\n";
    std::fs::write(&module, source).unwrap();
    let note = dir.path().join("main.wtf");
    std::fs::write(&note, "https://docs.example/start\n").unwrap();
    let uri = Url::from_file_path(&note).unwrap();
    let module_uri = Url::from_file_path(&module).unwrap();
    let mut client = Lsp::start(dir.path());
    client.notify("textDocument/didOpen",json!({"textDocument":{"uri":uri,"languageId":"wtf","version":1,"text":"https://docs.example/start\n"}}));
    let params = json!({"textDocument":{"uri":uri},"range":{"start":{"line":0,"character":0},"end":{"line":1,"character":0}}});
    client.diagnostics(1);
    client.wait_for_request("workspace/inlayHint/refresh");
    assert_eq!(
        client.request("textDocument/inlayHint", params.clone())[0]["label"],
        "first"
    );
    for (source, label) in [
        (source.replace("first", "second"), "second"),
        ("module := {".into(), "second"),
    ] {
        std::fs::write(&module, source).unwrap();
        client.notify(
            "workspace/didChangeWatchedFiles",
            json!({"changes":[{"uri":module_uri,"type":2}]}),
        );
        client.wait_for_request("workspace/inlayHint/refresh");
        assert_eq!(
            client.request("textDocument/inlayHint", params.clone())[0]["label"],
            label
        );
    }
    std::fs::remove_file(&module).unwrap();
    client.notify(
        "workspace/didChangeWatchedFiles",
        json!({"changes":[{"uri":module_uri,"type":3}]}),
    );
    client.wait_for_request("workspace/inlayHint/refresh");
    assert_eq!(client.request("textDocument/inlayHint", params), json!([]));
}

#[test]
fn module_reload_rejects_an_in_flight_refresh_before_saving_resources() {
    let dir = tempfile::tempdir().unwrap();
    let module_dir = dir.path().join(".wtf/modules");
    std::fs::create_dir_all(&module_dir).unwrap();
    let module = module_dir.join("docs.wtf");
    let script = dir.path().join("resolve.sh");
    std::fs::write(&script,"touch \"$1/started\"\nwhile [ ! -e \"$1/finish\" ]; do sleep 0.02; done\nprintf '{\"title\":\"stale\"}'\n").unwrap();
    let source = format!(
        "module := {{api: 1, id: \"docs\", kind: \"link\", hosts: [\"docs.example\"]}}\ninlay := fn(ctx) => \"first\"\nrefresh := fn(url) => {{program: \"/bin/sh\", args: [{}, {}]}}\ndecode := fn(url, data) => data\n",
        json!(script),
        json!(dir.path())
    );
    std::fs::write(&module, &source).unwrap();
    std::fs::write(dir.path().join("main.wtf"), "https://docs.example/start\n").unwrap();
    let mut client = Lsp::start(dir.path());
    client.request("wtf/query", json!({"query":"notes | count"}));
    client.next += 1;
    let id = client.next;
    client.send(json!({"jsonrpc":"2.0","id":id,"method":"workspace/executeCommand","params":{"command":"wtf.refresh","arguments":[]}}));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !dir.path().join("started").exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "Refresh never started"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    std::fs::write(&module, source.replace("first", "second")).unwrap();
    client.notify(
        "workspace/didChangeWatchedFiles",
        json!({"changes":[{"uri":Url::from_file_path(&module).unwrap(),"type":2}]}),
    );
    client.wait_for_request("workspace/inlayHint/refresh");
    std::fs::write(dir.path().join("finish"), "").unwrap();
    loop {
        let response = client.receive();
        if response["id"] == id {
            assert!(
                response["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("Modules changed"),
                "{response}"
            );
            break;
        }
    }
    assert!(
        !dir.path().join(".wtf/cache.json").exists(),
        "An outdated refresh wrote the resource cache"
    );
}

#[test]
fn module_edits_use_native_apply_edit_and_reject_stale_controls() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let modules = root.join(".wtf/modules");
    std::fs::create_dir_all(&modules).unwrap();
    std::fs::write(modules.join("edit.wtf"),r#"module := {api: 1, id: "edit", kind: "feature", inputs: []}
actions := fn(ctx) => if(ctx.row == 0, [{title: "Greeting", action: {kind: "edit", document: ctx.document.uri, expected: ctx.document.text, edits: [{range: {start: {line: 0, character: 0}, end: {line: 0, character: 5}}, newText: "Goodbye"}]}}], [])
"#).unwrap();
    let note = root.join("main.wtf");
    std::fs::write(&note, "Hello world\n").unwrap();
    let uri = Url::from_file_path(&note).unwrap();
    let mut client = Lsp::start(&root);
    client.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"wtf","version":1,"text":"Hello world\n"}}),
    );
    client.diagnostics(1);
    let lenses = client.request("textDocument/codeLens", json!({"textDocument":{"uri":uri}}));
    let command = lenses
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["command"]["title"] == "Greeting")
        .unwrap()["command"]
        .clone();
    assert_eq!(command["command"], "wtf.applyEdits");
    client.request("workspace/executeCommand", command.clone());
    assert_eq!(client.applied_edits.len(), 1);
    assert_eq!(
        client.applied_edits[0]["documentChanges"][0]["edits"][0]["newText"],
        "Goodbye"
    );
    client.notify("textDocument/didChange",json!({"textDocument":{"uri":uri,"version":2},"contentChanges":[{"text":"Goodbye world\n"}]}));
    client.diagnostics(2);
    let response = client.request_raw("workspace/executeCommand", command);
    assert!(
        response["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Source changed"),
        "{response}"
    );
    assert_eq!(client.applied_edits.len(), 1);
}

#[test]
fn file_queries_read_unsaved_syntax_and_keep_cross_file_graph_endpoints() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("one.wtf");
    let uri = Url::from_file_path(&path).unwrap();
    std::fs::write(&path, "1:saved\n").unwrap();
    std::fs::write(root.join("two.wtf"), "3:rate\n- [ ] Other\n").unwrap();
    let mut lsp = Lsp::start(&root);
    lsp.notify("textDocument/didOpen", json!({"textDocument":{"uri":uri,"languageId":"wtf","version":1,"text":"answer := rate * 2\n- [ ] Local\n"}}));
    assert_eq!(lsp.diagnostics(1), json!([]));
    let result = lsp.request("wtf/query", json!({"uri":uri,"query":"map(filter(ast, fn(n) => n.kind == \"definition\"), fn(n) => n.name)"}));
    assert_eq!(result["rows"], json!(["answer"]));
    assert_eq!(result["versions"][uri.as_str()], 1);
    assert_eq!(
        lsp.request(
            "wtf/query",
            json!({"uri":uri,"query":"tasks | select title"})
        )["rows"],
        json!(["Local"])
    );
    assert_eq!(
        lsp.request("wtf/query", json!({"query":"length(tasks)"}))["rows"],
        json!([2])
    );
    assert_eq!(
        lsp.request(
            "wtf/query",
            json!({"uri":uri,"query":"graph.nodes | where external | select name"})
        )["rows"],
        json!(["rate"])
    );
    for uri in [
        "https://example.com/n.wtf".to_string(),
        Url::from_file_path(root.join("missing.wtf"))
            .unwrap()
            .to_string(),
    ] {
        assert_eq!(
            lsp.request_raw("wtf/query", json!({"uri":uri,"query":"ast"}))["error"]["code"],
            -32602
        );
    }
    assert_eq!(std::fs::read_to_string(path).unwrap(), "1:saved\n");
}
