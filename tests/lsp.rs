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
            json!(jot::highlighting::TOKEN_TYPES)
        );
        assert_eq!(
            result["capabilities"]["semanticTokensProvider"]["legend"]["tokenModifiers"],
            json!(jot::highlighting::TOKEN_MODIFIERS)
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
        let text = jot::actions::apply_edits(text, &edits).unwrap();
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
    serde_json::to_value(jot::document::Span::new(row, start, start + needle.len()).range(text))
        .unwrap()
}

#[test]
fn raw_links_and_rich_tokens_work_over_lsp_and_follow_edits() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("links.jot");
    let uri = Url::from_file_path(&path).unwrap();
    let source = "🦀 ./packing.jot and https://example.com/docs.\n[focus] := countdown(25m)\n- [ ] Interview 09/17/2026 at 7AM\n";
    let mut lsp = Lsp::start(&root);
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"jot","version":1,"text":source}}),
    );
    assert_eq!(lsp.diagnostics(1), json!([]));
    let links = lsp.request(
        "textDocument/documentLink",
        json!({"textDocument":{"uri":uri}}),
    );
    assert_eq!(links.as_array().unwrap().len(), 2);
    assert_eq!(
        links[0]["target"],
        json!(Url::from_file_path(root.join("packing.jot")).unwrap())
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
        jot::highlighting::semantic_tokens(&jot::document::Document::parse(source.into()));
    assert_eq!(
        data,
        serde_json::to_value(lsp_types::SemanticTokens {
            result_id: None,
            data: expected
        })
        .unwrap()
    );
    let changed = "🦀 ../other.jot\n[focus] := $3.30\n- [x] Interview 09/17/2026 at 7AM\n";
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
    assert!(links[0]["target"].as_str().unwrap().ends_with("/other.jot"));
    let data = lsp.request(
        "textDocument/semanticTokens/full",
        json!({"textDocument":{"uri":uri}}),
    );
    let expected =
        jot::highlighting::semantic_tokens(&jot::document::Document::parse(changed.into()));
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
fn document_symbols_use_shared_hierarchy_and_follow_unsaved_edits() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("symbols.jot");
    let uri = Url::from_file_path(&path).unwrap();
    let text = "# Trip\n[$3,000]:budget\n## Money\n[remaining] := budget - $2,444\n- [ ] Pack :pack\n  - [x] Passport\n[focus] := countdown(25m)\n";
    std::fs::write(&path, text).unwrap();
    let mut lsp = Lsp::start(&root);
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"jot","version":1,"text":text}}),
    );
    assert_eq!(lsp.diagnostics(1), json!([]));
    let symbols = lsp.request(
        "textDocument/documentSymbol",
        json!({"textDocument":{"uri":uri}}),
    );
    let ws = jot::workspace::Workspace {
        roots: vec![root],
        documents: [(path.clone(), jot::document::Document::parse(text.into()))].into(),
        cache: Default::default(),
        lookups: Default::default(),
    };
    let shared = jot::symbols::document_symbols(&ws, &path, chrono::Local::now().fixed_offset());
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
    let path = root.join("symbols.jot");
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
    let data_path = root.join("data.jot");
    let calc_path = root.join("calc.jot");
    let data_uri = Url::from_file_path(&data_path).unwrap();
    let calc_uri = Url::from_file_path(&calc_path).unwrap();
    let data = "# Groceries\n[groceries] := table\n|item|quantity|price|\n|---|---|---|\n|apple|2|$3.30|\n|pear|4|$4.30|\n";
    let calc = "[total] := sum(groceries, quantity * price)\nCost [total].\n";
    std::fs::write(&data_path, data).unwrap();
    std::fs::write(&calc_path, calc).unwrap();
    let mut lsp = Lsp::start(&root);
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":data_uri,"languageId":"jot","version":1,"text":data}}),
    );
    assert_eq!(lsp.diagnostics(1), json!([]));
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":calc_uri,"languageId":"jot","version":1,"text":calc}}),
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
    let formatted = jot::actions::apply_edits(data, &edits).unwrap();
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
    let path = root.join("trip.jot");
    let uri = Url::from_file_path(&path).unwrap();
    let text = "# Trip\n\nOur budget is [$3,000]:budget.\n\nWe've spent [$2,444]:spent.\n\n[remaining] := budget - spent\n\nWe have [remaining] remaining.\n";
    std::fs::write(&path, text).unwrap();
    let mut lsp = Lsp::start(&root);
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"jot","version":1,"text":text}}),
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
    let path = root.join("interactions.jot");
    let uri = Url::from_file_path(&path).unwrap();
    let text = "[$3,000]:budget\n[$1,410]:spent\n[cash] := budget - spent\nUse [cash].\n[focus] := countdown(25m, 0s)\n- [ ] Review :review @timer(focus)\nPlain prose.\n";
    std::fs::write(&path, text).unwrap();
    let mut lsp = Lsp::start(&root);
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"jot","version":1,"text":text}}),
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
    let extracted = jot::actions::apply_edits(text, &edits).unwrap();
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
    let fixed = jot::actions::apply_edits(&typo, &edits).unwrap();
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
    let path = root.join("controls.jot");
    let uri = Url::from_file_path(&path).unwrap();
    let text = "- [ ] First :first\n[./receipt.png]:receipt\nUse [receipt].\n";
    std::fs::write(&path, text).unwrap();
    let mut lsp = Lsp::start(&root);
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"jot","version":1,"text":text}}),
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
    let completed = jot::actions::apply_edits(text, &edits).unwrap();
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
        !root.join(".jot").exists(),
        "Opening resources must not trigger a GitHub fetch/cache write"
    );
}

#[test]
fn live_diagnostics_and_codelenses_refresh_without_source_edits() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("live.jot");
    let uri = Url::from_file_path(&path).unwrap();
    let start = (chrono::Local::now() + chrono::Duration::seconds(1)).to_rfc3339();
    let text = format!("[focus] := countdown(3s, 0s, {start})\n[rate] := 1s / focus.elapsed\n");
    std::fs::write(&path, &text).unwrap();
    let mut lsp = Lsp::start(&root);
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"jot","version":1,"text":text}}),
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
    let path = root.join("plain.jot");
    let uri = Url::from_file_path(&path).unwrap();
    let text = "[value] := cou\n";
    std::fs::write(&path, text).unwrap();
    let mut lsp = Lsp::start_with_capabilities(&root, json!({}));
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"jot","version":1,"text":text}}),
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

#[test]
fn on_type_formatting_and_call_hierarchy_expose_the_dependency_graph() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let path = root.join("plan.jot");
    let uri = Url::from_file_path(&path).unwrap();
    let text = "[$3,000]:budget\n[$1,410]:spent\n[cash] := budget - spent\n[half] := cash / 2\n[t] := table\n| item | qty |\n|---|---|\n| apple | 2 |\n# Plan :plan\n- [ ] Buy :buy @after(cash > spent)\n\n";
    std::fs::write(&path, text).unwrap();
    let mut lsp = Lsp::start(&root);
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"jot","version":1,"text":text}}),
    );
    assert_eq!(lsp.diagnostics(1), json!([]));

    // Enter at the end of the task line continues the checklist.
    let edits = lsp.request(
        "textDocument/onTypeFormatting",
        json!({"textDocument":{"uri":uri},"position":{"line":10,"character":0},"ch":"\n","options":{"tabSize":2,"insertSpaces":true}}),
    );
    let edits: Vec<TextEdit> = serde_json::from_value(edits).unwrap();
    let continued = jot::actions::apply_edits(text, &edits).unwrap();
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
    let aligned = jot::actions::apply_edits(text, &edits).unwrap();
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
    let path = root.join("bakery.jot");
    let uri = Url::from_file_path(&path).unwrap();
    let text = "[bakery] := maximize($3 * bagels + $1.25 * doughnuts)\n| constraint | expression |\n| ---------- | ---------- |\n| flour | 12 * bagels + 6.5 * doughnuts <= 400 |\n| bagel_min | bagels >= 12 |\n| doughnut_min | doughnuts >= 14 |\nBake [bakery.bagels] bagels.\n";
    std::fs::write(&path, text).unwrap();
    let mut lsp = Lsp::start(&root);
    lsp.notify(
        "textDocument/didOpen",
        json!({"textDocument":{"uri":uri,"languageId":"jot","version":1,"text":text}}),
    );
    assert_eq!(lsp.diagnostics(1), json!([]));
    let range = json!({"start":{"line":0,"character":0},"end":{"line":20,"character":0}});
    let hints = lsp.request(
        "textDocument/inlayHint",
        json!({"textDocument":{"uri":uri},"range":range}),
    );
    assert_eq!(hints[0]["label"], "= $94.75 · bagels 25.75 · doughnuts 14");
    assert!(hints.to_string().contains("400 ≤ 400 · binding"));
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
    let renamed = jot::actions::apply_edits(text, &edits).unwrap();
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
