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
        modules: Default::default(),
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
    let sources=[("/notes/.wtf/modules/consumer.wtf".into(),"module := {api: 1, id: \"consumer\", kind: \"library\", imports: [\"math\"]}\nmath := import(\"math\")\nbase := 100\nrun := fn(x) => math.add(x)(2)\n".into()),("/notes/.wtf/modules/math.wtf".into(),"module := {api: 1, id: \"math\", kind: \"library\"}\nbase := 7\nadd := fn(x) => fn(y) => x + y + base\nloop := fn(x) => loop(x)\n".into())].into();
    ws.modules = Arc::new(wtf::modules::ModuleRegistry::compile(sources).unwrap());
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
            "/notes/.wtf/modules/a.wtf".into(),
            "module := {api: 1, id: \"a\", kind: \"library\", imports: [\"b\"]}".into(),
        ),
        (
            "/notes/.wtf/modules/b.wtf".into(),
            "module := {api: 1, id: \"b\", kind: \"library\", imports: [\"a\"]}".into(),
        ),
    ]
    .into();
    assert!(
        wtf::modules::ModuleRegistry::compile(cycle)
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
    let source = r#"module := {api: 1, id: "stamp", kind: "feature", inputs: []}
actions := fn(ctx) => [{title: "Stamp", action: {kind: "invoke", document: ctx.document.uri, expected: ctx.document.text, module: ctx.module.id, revision: ctx.module.revision, event: {}}}]
reduce := fn(ctx, event) => {kind: "edit", document: ctx.document.uri, expected: ctx.document.text, edits: [{range: {start: {line: 0, character: 0}, end: {line: 0, character: 0}}, newText: source(now())}]}
"#;
    let mut ws = note("Hello\n");
    let path = Path::new("/notes/features.wtf");
    let compile = |source: &str| {
        std::sync::Arc::new(
            wtf::modules::ModuleRegistry::compile(
                [("/notes/.wtf/modules/stamp.wtf".into(), source.into())].into(),
            )
            .unwrap(),
        )
    };
    ws.modules = compile(source);
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
    ws.modules = compile(&source.replace("Stamp", "New stamp"));
    assert!(
        action
            .prepare(&wtf::RequestContext::new(&ws, later), Capabilities::BROWSER)
            .unwrap_err()
            .contains("Module changed")
    );
}
#[test]
fn generic_hooks_validate_ranges_and_share_catalog_without_recursion() {
    let mut ws = note("Hello\n");
    let path = Path::new("/notes/features.wtf");
    let source = r#"module := {api: 1, id: "hooks", kind: "feature", inputs: ["diagnostics"]}
hovers := fn(ctx) => [{range: {start: {line: 0, character: 0}, end: {line: 0, character: 5}}, contents: "Greeting"}]
diagnostics := fn(ctx) => [{range: {start: {line: 0, character: 0}, end: {line: 0, character: 5}}, severity: 2, message: "Check greeting", code: "greeting"}]
format := fn(ctx) => [{range: {start: {line: 0, character: 0}, end: {line: 0, character: 5}}, newText: "HELLO"}]
"#;
    ws.modules = std::sync::Arc::new(
        wtf::modules::ModuleRegistry::compile(
            [("/notes/.wtf/modules/hooks.wtf".into(), source.into())].into(),
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

#[test]
fn migrated_features_can_be_disabled_without_leaking_controls_or_hooks() {
    let path = Path::new("/notes/features.wtf");
    let mut ws = note("watch := stopwatch()\n## Monday, September 16, 2026\n9:00 > Leave\n");
    let request = wtf::RequestContext::new(&ws, now());
    assert!(
        wtf::interaction::row_commands_in(&request, path, 0, false)
            .iter()
            .any(|c| c.title == "Start timer 'watch'")
    );
    assert!(!wtf::diagnostics::collect_in(&request, path, false).is_empty());
    assert!(wtf::features::module_features::hover(&request, path, Position::new(2, 2)).is_some());
    assert!(
        !wtf::features::module_features::formatting(&request, path)
            .unwrap()
            .is_empty()
    );
    ws.modules = std::sync::Arc::new(
        wtf::modules::ModuleRegistry::compile(
            ["timers", "itinerary"]
                .into_iter()
                .map(|id| {
                    (
                        format!("/notes/.wtf/modules/{id}.wtf").into(),
                        format!(
                            "module := {{api: 1, id: \"{id}\", kind: \"feature\", enabled: false}}\n"
                        ),
                    )
                })
                .collect(),
        )
        .unwrap(),
    );
    let request = wtf::RequestContext::new(&ws, now());
    assert!(wtf::interaction::row_commands_in(&request, path, 0, false).is_empty());
    assert!(wtf::diagnostics::collect_in(&request, path, false).is_empty());
    assert!(wtf::features::module_features::hover(&request, path, Position::new(2, 2)).is_none());
    assert!(
        wtf::features::module_features::formatting(&request, path)
            .unwrap()
            .is_empty()
    );
    assert!(
        wtf::presentation::hints_in(
            &request,
            path,
            Range::new(Position::new(0, 0), Position::new(9, 0))
        )
        .hints
        .is_empty()
    );
}

#[test]
fn language_diagnostics_reach_queries_and_imported_libraries_can_be_replaced() {
    let path = Path::new("/notes/features.wtf");
    let mut ws = note("## Monday, September 16, 2026\n9:00 > Leave\n");
    let query = wtf::query::Query::parse("diagnostics | where code == \"itinerary\"").unwrap();
    let rows = wtf::query::execute(&ws, &query, &wtf::query::QueryContext::new(now())).unwrap();
    assert_eq!(rows.rows.len(), 1);
    // Copying a bundled provider and replacing its imported library works through
    // the public registry, without registering a new Rust feature implementation.
    ws.modules = std::sync::Arc::new(wtf::modules::ModuleRegistry::compile([
        ("/notes/.wtf/modules/timers.wtf".into(), include_str!("../stdlib/timers.wtf").into()),
        ("/notes/.wtf/modules/timer.wtf".into(), "module := {api: 1, id: \"timer\", kind: \"library\", inputs: []}\nrunning := fn(t) => false\ninlay := fn(t) => \"custom timer\"\nactions := fn(t) => []\n".into()),
    ].into()).unwrap());
    ws.documents.insert(
        path.into(),
        Document::parse("watch := stopwatch()\n".into()),
    );
    let request = wtf::RequestContext::new(&ws, now());
    let hints = wtf::presentation::hints_in(
        &request,
        path,
        Range::new(Position::new(0, 0), Position::new(9, 0)),
    );
    assert!(
        serde_json::to_string(&hints.hints)
            .unwrap()
            .contains("custom timer")
    );
    assert!(wtf::interaction::row_commands_in(&request, path, 0, false).is_empty());
}

#[test]
fn solver_primitive_checks_models_and_keeps_integer_and_status_semantics() {
    let ws = note("");
    let path = Path::new("/notes/features.wtf");
    let mut engine = Engine::at(&ws, now());
    let model = r#"{goal: "maximize", variables: {x: {kind: "integer", lower: 0, upper: 2.8}}, objective: {constant: 0, terms: {x: 1}}, constraints: []}"#;
    let solve = |model: &str| format!("solve_linear({model})");
    let value = engine.eval(path, &solve(model)).unwrap();
    let json = wtf::modules::json(&value).unwrap();
    assert_eq!(json["status"], "optimal");
    assert_eq!(json["values"]["x"], 2);
    let unbounded = model
        .replace(", upper: 2.8", "")
        .replace("integer", "continuous");
    assert_eq!(
        wtf::modules::json(&engine.eval(path, &solve(&unbounded)).unwrap()).unwrap()["status"],
        "unbounded"
    );
    let infeasible = model.replace("constraints: []", "constraints: [{lhs: {constant: 0, terms: {x: 1}}, op: \">=\", rhs: {constant: 5, terms: {}}}]");
    assert_eq!(
        wtf::modules::json(&engine.eval(path, &solve(&infeasible)).unwrap()).unwrap()["status"],
        "infeasible"
    );
    for bad in [
        model.replace("lower: 0", "lower: 5"),
        model.replace("terms: {x: 1}", "terms: {missing: 1}"),
        model.replace("integer", "imaginary"),
        model.replace("maximize", "guess"),
        model.replace("constraints: []", "constraints: [], order: [\"x\", \"x\"]"),
    ] {
        assert!(engine.eval(path, &solve(&bad)).is_err(), "{bad}");
    }
}

#[test]
fn timers_keep_argument_validation_and_calendar_primitives_handle_invalid_input() {
    let ws = note("");
    let path = Path::new("/notes/features.wtf");
    let mut engine = Engine::at(&ws, now());
    for source in [
        "stopwatch(null)",
        "stopwatch(0s, null)",
        "countdown(1m, null)",
        "countdown(1m, 0s, null)",
    ] {
        assert!(engine.eval(path, source).is_err(), "{source}");
    }
    for source in [
        r#"parse_date("2026-02-29", "%F")"#,
        r#"parse_datetime("2026-09-18 25:00", "%F %H:%M", now())"#,
    ] {
        assert_eq!(engine.eval(path, source).unwrap(), wtf::engine::Value::Null);
    }
    assert_eq!(
        engine
            .eval(
                path,
                r#"source(parse_datetime("2026-09-18 09:30", "%F %H:%M", now()))"#
            )
            .unwrap()
            .display(),
        "2026-09-18T09:30:00-04:00"
    );
}

#[test]
fn representative_document_stays_within_module_limits() {
    let path = Path::new("/notes/features.wtf");
    let mut source = (0..32)
        .map(|n| format!("timer_{n} := countdown(25m, 0s, 2026-09-16T13:59:30-04:00)\n"))
        .collect::<String>();
    source.push_str("## Wednesday, September 16, 2026\n");
    for n in 0..20 {
        source.push_str(&format!("{:02}:00 > Stop {n}\n", n));
    }
    let ws = note(&source);
    let request = wtf::RequestContext::new(&ws, now());
    let started = std::time::Instant::now();
    let hints = wtf::presentation::hints_in(
        &request,
        path,
        Range::new(Position::new(0, 0), Position::new(100, 0)),
    );
    let diagnostics = wtf::diagnostics::collect_in(&request, path, false);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert_eq!(hints.hints.len(), 52, "{:?}", hints.hints);
    assert!(
        !serde_json::to_string(&hints.hints)
            .unwrap()
            .contains("module error")
    );
    eprintln!(
        "32 timers + 20 stops: {:?} for inlays and diagnostics",
        started.elapsed()
    );
}

#[test]
fn oversized_itinerary_reports_a_module_error_instead_of_panicking() {
    let mut source = "## September 16, 2026\n09:00 > Leave\n".to_string();
    source.push_str(&"Note: extra detail\n".repeat(2000));
    let ws = note(&source);
    let path = Path::new("/notes/features.wtf");
    let request = wtf::RequestContext::new(&ws, now());
    let issues = wtf::diagnostics::collect_in(&request, path, false);
    assert!(
        issues.iter().any(|d| d.message.contains("size limit")),
        "{issues:?}"
    );
    assert!(wtf::features::module_features::formatting(&request, path).is_err());
    assert!(
        wtf::intelligence::stop_hover(&ws, path, Position::new(1, 0), now().date_naive()).is_none()
    );
}

#[test]
fn timer_clock_preserves_integer_second_precision() {
    for seconds in [59, 3600, 9_007_199_254_741_003, i64::MAX] {
        let timer =
            wtf::timers::Timer::new("stopwatch", &[wtf::engine::Value::Duration(seconds)], now())
                .unwrap();
        let expected = if seconds < 3600 {
            format!("{:02}:{:02}", seconds / 60, seconds % 60)
        } else {
            format!(
                "{:02}:{:02}:{:02}",
                seconds / 3600,
                seconds / 60 % 60,
                seconds % 60
            )
        };
        assert!(
            timer.display().contains(&expected),
            "{} vs {expected}",
            timer.display()
        );
    }
}

#[test]
fn plans_preserve_source_variable_order() {
    let ws = note(
        "result := maximize(zebra + apple)\n| constraint | expression |\n| --- | --- |\n| z | zebra <= 1 |\n| a | apple <= 2 |\n",
    );
    let path = Path::new("/notes/features.wtf");
    let request = wtf::RequestContext::new(&ws, now());
    let hints = wtf::presentation::hints_in(
        &request,
        path,
        Range::new(Position::new(0, 0), Position::new(9, 0)),
    );
    let json = serde_json::to_value(hints.hints).unwrap();
    assert_eq!(json[0]["label"], "= 3 · zebra 1 · apple 2");
    let symbol = ws.resolve(path, "result").unwrap();
    assert!(
        wtf::intelligence::hover_in(&request, &symbol).contains("Variables: zebra = 1, apple = 2")
    );
}
