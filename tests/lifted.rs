use chrono::{DateTime, FixedOffset};
use lsp_types::{Position, Range};
use serde_json::{Value, json};
use std::path::Path;
use wtf::{
    document::Document,
    engine::Engine,
    workspace::{Symbol, SymbolKind, Workspace},
};
fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-09-16T14:00:00-04:00").unwrap()
}
fn note(source: &str) -> Workspace {
    Workspace {
        roots: vec!["/notes".into()],
        documents: [("/notes/features.wtf".into(), Document::parse(source.into()))].into(),
        cache: Default::default(),
        lookups: Default::default(),
        plugins: Default::default(),
    }
}
fn assert_json(at: &str, actual: &Value, expected: &Value) {
    match (actual, expected) {
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{at}");
            for (i, (a, b)) in a.iter().zip(b).enumerate() {
                assert_json(&format!("{at}/{i}"), a, b);
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(
                a.keys().collect::<Vec<_>>(),
                b.keys().collect::<Vec<_>>(),
                "{at}"
            );
            for (k, a) in a {
                assert_json(&format!("{at}/{k}"), a, &b[k]);
            }
        }
        _ => assert_eq!(actual, expected, "{at}"),
    }
}
#[test]
fn feature_behavior_matches_native_baseline() {
    let path = Path::new("/notes/features.wtf");
    let mut output = vec![];
    for source in [
        "[400]:flour_stock\n[bakery] := maximize($3 * bagels + $1.25 * doughnuts)\n| constraint | expression |\n| --- | --- |\n| flour | 12 * bagels + 6.5 * doughnuts <= flour_stock |\n| minimum | bagels >= 12 |\n| doughnuts_min | doughnuts >= 14 |\nBake [bakery.bagels] for [bakery].\n",
        "items := table\n| item | cost | take? |\n| --- | --- | --- |\n| Tea | $3 | |\n| Pie | $5 | |\nchoice := maximize(sum(items, take * cost))\n| constraint | expression |\n| --- | --- |\n| budget | sum(items, take * cost) <= $5 |\n",
        "watch := stopwatch()\nfocus := countdown(2m, 30s, 2026-09-16T13:59:30-04:00)\npaused := stopwatch(65s)\ndone := countdown(1s, 1s)\n- [ ] Work @timer(focus)\nSpent [watch.elapsed], use [focus] and [paused].\n",
        "## Wednesday, September 16, 2026 · Oaxaca\n07:04 AM  > Depart\n11:55 AM  < Arrive\n02:45 PM  > Depart again\n06:00 PM  @ Check in\nAddress: Calle Main\nCancel by: 24h before\nA note\n## Thursday, September 17\n13:00 * Lunch\nCancel by: 2026-09-16 12:00\n",
        "## Monday, February 30, 2026\n09:00 Some stop\n## Monday, September 15, 2026\n15:00 > Leave\n14:00 < Arrive\n## January 1, 2026\n",
        "## December 31\n23:00 > Depart\n## January 1\n01:00 < Arrive\n",
    ] {
        let ws = note(source);
        let request = wtf::RequestContext::new(&ws, now());
        let mut engine = request.engine();
        let hints = wtf::presentation::hints_in(
            &request,
            path,
            Range::new(Position::new(0, 0), Position::new(99, 0)),
        )
        .hints;
        let mut definitions = vec![];
        for (i, def) in ws.documents[path].definitions.iter().enumerate() {
            let symbol = Symbol {
                path: path.into(),
                kind: SymbolKind::Definition(i),
            };
            definitions.push(json!({"name":def.named.name,"value":engine.symbol(&symbol).map(|v|v.display()),"hover":wtf::intelligence::hover_in(&request,&symbol)}));
        }
        let doc = &ws.documents[path];
        let stop_hovers: Vec<_> = doc
            .days
            .iter()
            .flat_map(|d| &d.stops)
            .map(|s| {
                wtf::intelligence::stop_hover(
                    &ws,
                    path,
                    Position::new(s.line as u32, 0),
                    now().date_naive(),
                )
            })
            .collect();
        output.push(json!({"source":source,"hints":hints,"definitions":definitions,"diagnostics":wtf::diagnostics::collect_in(&request,path,false),"format":wtf::tables::formatting(doc),"stop_hovers":stop_hovers}));
    }
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lifted-features.json");
    assert_json(
        "features",
        &json!(output),
        &serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap(),
    );
}
#[test]
fn primitive_linear_solver_and_imports_are_available_to_notes_and_queries() {
    let ws = note("fmt := import(\"format\")\nresult := fmt.human(125m)\n");
    let mut e = Engine::at(&ws, now());
    let path = Path::new("/notes/features.wtf");
    assert_eq!(e.named(path, "result").unwrap().display(), "2h 5m");
    let model = r#"solve_linear({goal: "maximize", variables: {x: {kind: "continuous", lower: 0}}, objective: {constant: 0, terms: {x: 3}}, constraints: [{lhs: {constant: 0, terms: {x: 1}}, op: "<=", rhs: {constant: 4, terms: {}}}]})"#;
    assert_eq!(
        e.eval(path, &format!("{model}.values.x"))
            .unwrap()
            .display(),
        "4"
    );
}

#[test]
fn imported_functions_keep_lexical_environments_and_share_execution_limits() {
    use std::sync::Arc;
    let mut ws = note("x := import(\"consumer\").run(3)\n");
    let sources=[("/notes/.wtf/plugins/consumer.wtf".into(),"plugin := {api: 1, id: \"consumer\", kind: \"library\", imports: [\"math\"]}\nmath := import(\"math\")\nbase := 100\nrun := fn(x) => math.add(x)(2)\n".into()),("/notes/.wtf/plugins/math.wtf".into(),"plugin := {api: 1, id: \"math\", kind: \"library\"}\nbase := 7\nadd := fn(x) => fn(y) => x + y + base\nloop := fn(x) => loop(x)\n".into())].into();
    ws.plugins = Arc::new(wtf::plugins::Plugins::compile(sources).unwrap());
    let mut engine = Engine::at(&ws, now());
    let path = Path::new("/notes/features.wtf");
    assert_eq!(engine.named(path, "x").unwrap().display(), "12");
    assert!(
        engine
            .eval(path, "import(\"math\").loop(0)")
            .unwrap_err()
            .contains("depth")
    );
    let cycle = [
        (
            "/notes/.wtf/plugins/a.wtf".into(),
            "plugin := {api: 1, id: \"a\", kind: \"library\", imports: [\"b\"]}".into(),
        ),
        (
            "/notes/.wtf/plugins/b.wtf".into(),
            "plugin := {api: 1, id: \"b\", kind: \"library\", imports: [\"a\"]}".into(),
        ),
    ]
    .into();
    assert!(
        wtf::plugins::Plugins::compile(cycle)
            .unwrap_err()
            .contains("cycle")
    );
    let q = wtf::query::Query::parse("notes | select import(\"format\").human(125m)").unwrap();
    assert_eq!(
        serde_json::to_value(
            wtf::query::execute(&ws, &q, &wtf::query::QueryContext::new(now())).unwrap()
        )
        .unwrap()["rows"],
        json!(["2h 5m"])
    );
}
#[test]
fn reducers_run_at_execution_time_and_reject_changed_modules() {
    use wtf::commands::{Action, Capabilities, PreparedAction};
    let source = r#"plugin := {api: 1, id: "stamp", kind: "inlay", inputs: []}
actions := fn(ctx) => [{title: "Stamp", action: {kind: "invoke", document: ctx.document.uri, expected: ctx.document.text, plugin: ctx.plugin.id, revision: ctx.plugin.revision, event: {}}}]
reduce := fn(ctx, event) => {kind: "edit", document: ctx.document.uri, expected: ctx.document.text, edits: [{range: {start: {line: 0, character: 0}, end: {line: 0, character: 0}}, newText: source(now())}]}
"#;
    let mut ws = note("Hello\n");
    let path = Path::new("/notes/features.wtf");
    let compile = |source: &str| {
        std::sync::Arc::new(
            wtf::plugins::Plugins::compile(
                [("/notes/.wtf/plugins/stamp.wtf".into(), source.into())].into(),
            )
            .unwrap(),
        )
    };
    ws.plugins = compile(source);
    let commands = wtf::interaction::row_commands_for(
        &wtf::RequestContext::new(&ws, now()),
        path,
        0,
        true,
        Capabilities::BROWSER,
    );
    let command = commands.iter().find(|c| c.title == "Stamp").unwrap();
    let action = Action::decode(&command.command, command.arguments.as_ref().unwrap()).unwrap();
    let later = now() + chrono::Duration::minutes(2);
    let PreparedAction::Edit { edits, .. } = action
        .prepare(&wtf::RequestContext::new(&ws, later), Capabilities::BROWSER)
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(edits[0].new_text, later.to_rfc3339());
    ws.plugins = compile(&source.replace("Stamp", "New stamp"));
    assert!(
        action
            .prepare(&wtf::RequestContext::new(&ws, later), Capabilities::BROWSER)
            .unwrap_err()
            .contains("Plugin changed")
    );
}
#[test]
fn generic_hooks_validate_ranges_and_share_catalog_without_recursion() {
    let mut ws = note("Hello\n");
    let path = Path::new("/notes/features.wtf");
    let source = r#"plugin := {api: 1, id: "hooks", kind: "inlay", inputs: ["diagnostics"]}
hovers := fn(ctx) => [{range: {start: {line: 0, character: 0}, end: {line: 0, character: 5}}, contents: "Greeting"}]
diagnostics := fn(ctx) => [{range: {start: {line: 0, character: 0}, end: {line: 0, character: 5}}, severity: 2, message: "Check greeting", code: "greeting"}]
format := fn(ctx) => [{range: {start: {line: 0, character: 0}, end: {line: 0, character: 5}}, newText: "HELLO"}]
"#;
    ws.plugins = std::sync::Arc::new(
        wtf::plugins::Plugins::compile(
            [("/notes/.wtf/plugins/hooks.wtf".into(), source.into())].into(),
        )
        .unwrap(),
    );
    let request = wtf::RequestContext::new(&ws, now());
    assert!(wtf::features::module_features::hover(&request, path, Position::new(0, 1)).is_some());
    assert_eq!(
        wtf::diagnostics::collect_in(&request, path, false)[0].message,
        "Check greeting"
    );
    let edits = wtf::features::module_features::formatting(&request, path).unwrap();
    assert_eq!(
        wtf::actions::apply_edits("Hello\n", &edits).unwrap(),
        "HELLO\n"
    );
}
