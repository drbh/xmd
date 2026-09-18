#![cfg(feature = "browser")]
use serde_json::{Value, json};
use wtf::browser::BrowserWorkspace;

const URI: &str = "file:///workspace/trip.wtf";
const NOW: &str = "2026-09-16T14:00:00-04:00";

#[test]
fn browser_raw_links_use_shared_lsp_targets_and_hovers() {
    let mut ws = BrowserWorkspace::new();
    let source = "🦀 ./today.wtf and https://example.com/docs.\n";
    set(&mut ws, URI, source, 1);
    let path = std::path::Path::new("/workspace/trip.wtf");
    let shared = wtf::workspace::Workspace {
        roots: vec!["/workspace".into()],
        documents: [(path.into(), wtf::document::Document::parse(source.into()))].into(),
        cache: Default::default(),
        lookups: Default::default(),
    };
    let expected = serde_json::to_value(wtf::presentation::document_links(
        &shared,
        path,
        chrono::DateTime::parse_from_rfc3339(NOW).unwrap(),
    ))
    .unwrap();
    let links = request(&mut ws, "documentLinks", json!({"uri":URI}));
    assert_eq!(links, expected);
    assert_eq!(
        request(&mut ws, "analyze", json!({"uri":URI}))["links"],
        expected
    );
    assert_eq!(links[0]["target"], "file:///workspace/today.wtf");
    assert_eq!(links[0]["range"]["start"]["character"], 3);
    let hover = request(
        &mut ws,
        "hover",
        json!({"uri":URI,"position":{"line":0,"character":4}}),
    );
    assert!(
        hover["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("file:///workspace/today.wtf")
    );
}
fn raw(ws: &mut BrowserWorkspace, method: &str, params: Value, now: &str) -> Value {
    serde_json::from_str(&ws.request(method, &params.to_string(), now)).unwrap()
}
fn request(ws: &mut BrowserWorkspace, method: &str, params: Value) -> Value {
    let response = raw(ws, method, params, NOW);
    assert_eq!(response["ok"], true, "{response}");
    response["result"].clone()
}
fn set(ws: &mut BrowserWorkspace, uri: &str, text: &str, version: i32) {
    request(
        ws,
        "setDocument",
        json!({"uri":uri,"text":text,"version":version}),
    );
}

#[test]
fn browser_highlighting_uses_shared_lsp_legend_modifiers_and_tokens() {
    let mut ws = BrowserWorkspace::new();
    let legend = request(&mut ws, "semanticLegend", json!({}));
    assert_eq!(legend["tokenTypes"], json!(wtf::highlighting::TOKEN_TYPES));
    assert_eq!(
        legend["tokenModifiers"],
        json!(wtf::highlighting::TOKEN_MODIFIERS)
    );
    for (i, source) in [
        "[value] := $3.30\nHave [value].",
        "[value] := 2026-09-16\nHave [value].",
    ]
    .iter()
    .enumerate()
    {
        set(&mut ws, URI, source, i as i32 + 1);
        let snapshot = request(&mut ws, "analyze", json!({"uri":URI}));
        let doc = wtf::document::Document::parse((*source).into());
        let expected: Vec<_> = wtf::highlighting::semantic_tokens(&doc)
            .into_iter()
            .flat_map(|t| {
                [
                    t.delta_line,
                    t.delta_start,
                    t.length,
                    t.token_type,
                    t.token_modifiers_bitset,
                ]
            })
            .collect();
        assert_eq!(snapshot["tokens"], json!(expected));
    }
}

#[test]
fn browser_document_symbols_are_the_standard_shared_lsp_data() {
    let source = "# Trip\n[$3,000]:budget\n## Money\n[remaining] := budget - $2,444\n- [ ] Pack\n  - [x] Passport\n";
    let mut ws = BrowserWorkspace::new();
    set(&mut ws, URI, source, 1);
    let result = request(&mut ws, "documentSymbols", json!({"uri":URI}));
    let path = std::path::Path::new("/workspace/trip.wtf");
    let shared = wtf::workspace::Workspace {
        roots: vec!["/workspace".into()],
        documents: [(path.into(), wtf::document::Document::parse(source.into()))].into(),
        cache: Default::default(),
        lookups: Default::default(),
    };
    assert_eq!(
        result,
        serde_json::to_value(wtf::symbols::document_symbols(
            &shared,
            path,
            chrono::DateTime::parse_from_rfc3339(NOW).unwrap()
        ))
        .unwrap()
    );
    assert_eq!(
        request(&mut ws, "analyze", json!({"uri":URI}))["symbols"],
        result
    );
}

#[test]
fn browser_tables_share_types_column_targets_and_formatting_edits() {
    let mut ws = BrowserWorkspace::new();
    let text = "[fruit] := table\n|name|qty|price|\n|---|---|---|\n|apple|2|$3.30|\n|pear|4|$4.30|\n[cost] := sum(fruit, qty * price)\nCost [cost].\n";
    set(&mut ws, URI, text, 1);
    let result = request(&mut ws, "analyze", json!({"uri":URI}));
    assert_eq!(result["diagnostics"], json!([]));
    assert_eq!(result["hints"][1]["label"], "= $23.80");
    let reference = json!({"uri":URI,"position":{"line":5,"character":29}});
    let definition = request(&mut ws, "definition", reference.clone());
    assert_eq!(definition["range"]["start"]["line"], 1);
    let mut rename = reference;
    rename["newName"] = json!("unit_price");
    let edits = request(&mut ws, "rename", rename);
    assert_eq!(
        edits["documentChanges"][0]["edits"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(edits["documentChanges"][0]["textDocument"]["version"], 1);
    let formatted = request(&mut ws, "formatting", json!({"uri":URI}));
    assert_eq!(
        formatted,
        serde_json::to_value(wtf::tables::formatting(&wtf::document::Document::parse(
            text.into()
        )))
        .unwrap()
    );
}

#[test]
fn browser_workspace_shares_inlays_diagnostics_and_cross_note_navigation() {
    let mut ws = BrowserWorkspace::new();
    set(
        &mut ws,
        URI,
        "[cash] := budget - spent\n🦀 Have [cash].\n",
        1,
    );
    set(
        &mut ws,
        "file:///workspace/values.wtf",
        "[$3,000]:budget\n[$2,444]:spent\n",
        1,
    );
    let snapshot = request(&mut ws, "analyze", json!({"uri":URI}));
    assert_eq!(snapshot["diagnostics"], json!([]));
    assert_eq!(snapshot["hints"][0]["label"], "= $556");
    assert_eq!(snapshot["hints"][1]["label"], "$556");
    assert_eq!(snapshot["hints"][1]["position"]["character"], 14);
    assert!(!snapshot["tokens"].as_array().unwrap().is_empty());
    let definition = request(
        &mut ws,
        "definition",
        json!({"uri":URI,"position":{"line":0,"character":12}}),
    );
    assert_eq!(definition["uri"], "file:///workspace/values.wtf");
    let renamed = request(
        &mut ws,
        "rename",
        json!({"uri":URI,"position":{"line":0,"character":12},"newName":"trip_budget"}),
    );
    assert_eq!(renamed["documentChanges"].as_array().unwrap().len(), 2);
    assert!(
        renamed["documentChanges"]
            .as_array()
            .unwrap()
            .iter()
            .all(|d| d["textDocument"]["version"] == 1)
    );
}

#[test]
fn browser_controls_are_undoable_snapshots_and_reject_stale_versions() {
    let mut ws = BrowserWorkspace::new();
    let text = "[focus] := countdown(2s)\n- [ ] Review @timer(focus)\n";
    set(&mut ws, URI, text, 1);
    let snapshot = request(&mut ws, "analyze", json!({"uri":URI}));
    let command = snapshot["lenses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["command"]["title"] == "Start timer 'focus'")
        .unwrap()["command"]
        .clone();
    let result = request(
        &mut ws,
        "execute",
        json!({"command":command,"versions":snapshot["versions"]}),
    );
    let edits: Vec<lsp_types::TextEdit> =
        serde_json::from_value(result["edit"]["documentChanges"][0]["edits"].clone()).unwrap();
    let running = wtf::actions::apply_edits(text, &edits).unwrap();
    assert!(running.contains("countdown(2s, 0s, 2026-09-16T14:00:00-04:00)"));
    // Returning an edit does not mutate source until the editor applies/reports it.
    assert_eq!(
        request(&mut ws, "analyze", json!({"uri":URI}))["version"],
        1
    );
    set(&mut ws, URI, &running, 2);
    let rejected = raw(
        &mut ws,
        "execute",
        json!({"command":command,"versions":snapshot["versions"]}),
        NOW,
    );
    assert_eq!(rejected["ok"], false);
    let finished = raw(
        &mut ws,
        "analyze",
        json!({"uri":URI}),
        "2026-09-16T14:00:03-04:00",
    );
    assert!(finished.to_string().contains("00:00 remaining · ✓ done"));
    assert_eq!(finished["result"]["live"], false);
    assert_eq!(finished["result"]["version"], 2);
}

#[test]
fn browser_input_validation_never_escapes_virtual_workspace() {
    let mut ws = BrowserWorkspace::new();
    for uri in [
        "file:///etc/secrets.wtf",
        "https://example.com/note.wtf",
        "file:///workspace/../secret.wtf",
        "file:///workspace/test.wtf#L1",
    ] {
        assert_eq!(
            raw(
                &mut ws,
                "setDocument",
                json!({"uri":uri,"text":"","version":1}),
                NOW
            )["ok"],
            false
        );
    }
    set(
        &mut ws,
        URI,
        "[https://github.com/zed-industries/zed/pull/1]:pr\n",
        2,
    );
    assert_eq!(
        raw(
            &mut ws,
            "setDocument",
            json!({"uri":URI,"text":"oops","version":1}),
            NOW
        )["ok"],
        false
    );
    let snapshot = request(&mut ws, "analyze", json!({"uri":URI}));
    assert!(!snapshot.to_string().contains("wtf.refreshResource"));
    assert_eq!(
        serde_json::from_str::<Value>(&ws.request("analyze", "not JSON", NOW)).unwrap()["ok"],
        false
    );
}

#[test]
fn browser_on_type_formatting_shares_table_alignment_and_checklist_continuation() {
    let mut ws = BrowserWorkspace::new();
    let text = "[fruit] := table\n|name|qty|\n|---|---|\n|apple|2|\n- [ ] first\n\n";
    set(&mut ws, URI, text, 1);
    let edits = request(
        &mut ws,
        "onTypeFormatting",
        json!({"uri":URI,"position":{"line":3,"character":9},"ch":"|"}),
    );
    let edits: Vec<lsp_types::TextEdit> = serde_json::from_value(edits).unwrap();
    let aligned = wtf::actions::apply_edits(text, &edits).unwrap();
    assert!(
        aligned.contains("| name  | qty |\n| ----- | --- |\n| apple | 2   |\n"),
        "{aligned}"
    );
    let edits = request(
        &mut ws,
        "onTypeFormatting",
        json!({"uri":URI,"position":{"line":5,"character":0},"ch":"\n"}),
    );
    let edits: Vec<lsp_types::TextEdit> = serde_json::from_value(edits).unwrap();
    assert_eq!(
        wtf::actions::apply_edits(text, &edits).unwrap(),
        "[fruit] := table\n|name|qty|\n|---|---|\n|apple|2|\n- [ ] first\n- [ ] \n"
    );
}

#[test]
fn browser_plans_solve_in_the_shared_engine() {
    let mut ws = BrowserWorkspace::new();
    let text = "[bakery] := maximize($3 * bagels + $1.25 * doughnuts)\n| constraint | expression |\n| --- | --- |\n| flour | 12 * bagels + 6.5 * doughnuts <= 400 |\n| bagel_min | bagels >= 12 |\n| doughnut_min | doughnuts >= 14 |\nBake [bakery.bagels] bagels.\n";
    set(&mut ws, URI, text, 1);
    let result = request(&mut ws, "analyze", json!({"uri":URI}));
    assert_eq!(result["diagnostics"], json!([]));
    assert_eq!(
        result["hints"][0]["label"],
        "= $94.75 · bagels 25.75 · doughnuts 14"
    );
    assert_eq!(result["hints"][4]["label"], "25.75");
    let symbols = request(&mut ws, "documentSymbols", json!({"uri":URI}));
    assert_eq!(symbols[0]["children"][0]["name"], "bagels");
}
