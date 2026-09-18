//! These checks replace library exports, so a hidden native fallback cannot pass.
use chrono::{DateTime, FixedOffset};
use lsp_types::{Position, Range};
use std::{path::Path, sync::Arc};
use wtf::{
    RequestContext, document::Document, engine::Value, modules::ModuleRegistry,
    workspace::Workspace,
};

fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-09-16T14:00:00-04:00").unwrap()
}
fn path() -> &'static Path {
    Path::new("/notes/main.wtf")
}
fn note(source: &str) -> Workspace {
    Workspace {
        roots: vec!["/notes".into()],
        documents: [(path().into(), Document::parse(source.into()))].into(),
        cache: Default::default(),
        lookups: Default::default(),
        modules: Default::default(),
    }
}
fn replace(ws: &mut Workspace, id: &str, source: String) {
    ws.modules = Arc::new(
        ModuleRegistry::compile([(format!("/notes/.wtf/modules/{id}.wtf").into(), source)].into())
            .unwrap(),
    );
}
fn export(source: String, name: &str, function: &str) -> String {
    let needle = format!("{name} := fn(");
    assert!(source.contains(&needle), "missing export {name}");
    source.replacen(&needle, &format!("_{name} := fn("), 1) + &format!("\n{name} := {function}\n")
}
fn hints(ws: &Workspace) -> Vec<lsp_types::InlayHint> {
    wtf::presentation::hints_in(
        &RequestContext::new(ws, now()),
        path(),
        Range::new(Position::new(0, 0), Position::new(u32::MAX, u32::MAX)),
    )
    .hints
}

#[test]
fn disabling_the_feature_modules_removes_every_inlay_without_native_fallbacks() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/stdlib-html.json")).unwrap();
    let disabled = Arc::new(
        ModuleRegistry::compile(
            wtf::modules::bundled()
                .iter()
                .filter(|m| m.kind == "feature")
                .map(|m| {
                    (
                        format!("/notes/.wtf/modules/{}.wtf", m.id).into(),
                        format!(
                            "module := {{api: 1, id: \"{}\", kind: \"feature\", enabled: false}}",
                            m.id
                        ),
                    )
                })
                .collect(),
        )
        .unwrap(),
    );
    for source in fixtures
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["source"].as_str().unwrap())
        .chain(["https://github.com/openai/example/pull/42\n"])
    {
        let mut ws = note(source);
        assert!(!hints(&ws).is_empty(), "{source}");
        ws.modules = disabled.clone();
        assert!(hints(&ws).is_empty(), "{source}");
        assert!(
            !wtf::rendering::html_in(&RequestContext::new(&ws, now()), path())
                .unwrap()
                .contains("class=\"inlay\"")
        );
    }
}

#[test]
fn timer_constructor_properties_display_and_edits_use_the_replaced_library() {
    let mut ws = note("watch := stopwatch()\nElapsed [watch.elapsed].\n");
    let timer = export(
        include_str!("../stdlib/timer.wtf").into(),
        "create",
        "fn(kind, args) => {limit: null, elapsed: 41s, started: null, idle: false}",
    );
    let timer = export(timer, "property", "fn(t, name) => 42s");
    let timer = export(timer, "display", "fn(t) => \"CUSTOM TIMER\"");
    let timer = export(
        timer,
        "transition",
        "fn(t, action, original) => \"stopwatch(17s)\"",
    );
    replace(&mut ws, "timer", timer);
    let request = RequestContext::new(&ws, now());
    let Value::Timer(timer) = request.engine().named(path(), "watch").unwrap() else {
        panic!()
    };
    assert_eq!(timer.elapsed, 41);
    assert_eq!(timer.display(), "CUSTOM TIMER");
    assert_eq!(timer.property("elapsed").unwrap(), Value::Duration(42));
    assert_eq!(
        request.engine().eval(path(), "watch.elapsed").unwrap(),
        Value::Duration(42)
    );
    assert_eq!(
        wtf::timers::edit_in(&request, path(), "watch", wtf::timers::TimerAction::Resume)
            .unwrap()
            .1
            .new_text,
        " stopwatch(17s)"
    );
    let html = wtf::rendering::html_in(&request, path()).unwrap();
    assert!(html.contains("CUSTOM TIMER"));
    assert!(html.contains("42s</span>"));
    // Previously created values keep their immutable implementation snapshot.
    ws.modules = Default::default();
    assert_eq!(timer.display(), "CUSTOM TIMER");
    assert_ne!(
        RequestContext::new(&ws, now())
            .engine()
            .named(path(), "watch")
            .unwrap()
            .display(),
        "CUSTOM TIMER"
    );
}

#[test]
fn solver_policy_errors_reach_evaluation_and_resolved_html() {
    let mut ws = note(
        "best := maximize($2 * count)\n| constraint | expression |\n| --- | --- |\n| limit | count <= 3 |\n",
    );
    let plan = export(
        include_str!("../stdlib/plan.wtf").into(),
        "solve_model",
        "fn(model) => error(\"ACTIVE PLAN LIBRARY\")",
    );
    replace(&mut ws, "plan", plan);
    let request = RequestContext::new(&ws, now());
    assert!(
        request
            .engine()
            .named(path(), "best")
            .unwrap_err()
            .contains("ACTIVE PLAN LIBRARY")
    );
    let diagnostics = wtf::diagnostics::collect_in(&request, path(), true);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].message, "ACTIVE PLAN LIBRARY");
    assert_eq!(diagnostics[0].range.start.line, 0);
    assert!(
        wtf::rendering::html_in(&request, path())
            .unwrap()
            .contains("ACTIVE PLAN LIBRARY")
    );
}

#[test]
fn itinerary_library_changes_both_query_dates_and_editor_output() {
    let mut ws = note("## September 16, 2026\n09:00 > Leave\n10:00 < Arrive\n");
    let trip = export(
        include_str!("../stdlib/itinerary_core.wtf").into(),
        "dates",
        "fn(days, today) => map(days, fn(d) => 2040-01-02)",
    );
    let trip = export(trip, "time_text", "fn(stop) => \"CUSTOM TIME\"");
    replace(&mut ws, "itinerary_core", trip);
    let request = RequestContext::new(&ws, now());
    let query = wtf::query::Query::parse("stops | select at_date").unwrap();
    let result = wtf::query::execute(&ws, &query, &wtf::query::QueryContext::new(now())).unwrap();
    assert!(
        serde_json::to_string(&result.rows)
            .unwrap()
            .contains("2040-01-02")
    );
    let html = wtf::rendering::html_in(&request, path()).unwrap();
    assert!(html.contains("2040-01-02"));
    assert!(html.contains("CUSTOM TIME"));
    assert!(
        serde_json::to_string(&wtf::symbols::document_symbols_in(&request, path()))
            .unwrap()
            .contains("CUSTOM TIME")
    );
}

#[test]
fn the_units_library_converts_dimensions_and_reports_bad_input() {
    let ws = note("distance := import(\"units\").km_to_mi(100)\n");
    let engine = || RequestContext::new(&ws, now()).engine();
    for (expression, expected) in [
        ("import(\"units\").convert(100, \"c\", \"f\")", 212.0),
        (
            "import(\"units\").convert(212, \"fahrenheit\", \"celsius\")",
            100.0,
        ),
        ("import(\"units\").convert(0, \"c\", \"k\")", 273.15),
        ("import(\"units\").convert(1, \"mi\", \"ft\")", 5280.0),
        ("import(\"units\").convert(2, \"kg\", \"g\")", 2000.0),
        ("import(\"units\").convert(1, \"gal\", \"qt\")", 4.0),
        ("import(\"units\").convert(1, \"gb\", \"mb\")", 1000.0),
        ("import(\"units\").convert(1, \"ha\", \"sqm\")", 10000.0),
        ("import(\"units\").convert(1, \"mps\", \"kph\")", 3.6),
        (
            "import(\"units\").lb_to_kg(import(\"units\").kg_to_lb(5))",
            5.0,
        ),
    ] {
        let Value::Number(value) = engine().eval(path(), expression).unwrap() else {
            panic!("{expression} is not numeric");
        };
        assert!((value - expected).abs() < 1e-9, "{expression} gave {value}");
    }
    let Value::Number(miles) = engine().named(path(), "distance").unwrap() else {
        panic!("km_to_mi is not numeric");
    };
    assert!((miles - 62.137_119_223_733_39).abs() < 1e-9);
    assert_eq!(
        engine()
            .eval(
                path(),
                "import(\"units\").format(62.13711922373339, \"miles\")"
            )
            .unwrap(),
        Value::Text("62.14 mi".into())
    );
    assert_eq!(
        engine()
            .eval(path(), "import(\"units\").dimension(\"tbsp\")")
            .unwrap(),
        Value::Text("volume".into())
    );
    assert_eq!(
        engine()
            .eval(path(), "import(\"units\").convert(1, \"km\", \"kg\")")
            .unwrap_err(),
        "Cannot convert km to kg"
    );
    assert_eq!(
        engine()
            .eval(path(), "import(\"units\").convert(1, \"smoots\", \"m\")")
            .unwrap_err(),
        "Unknown unit 'smoots'"
    );
}

#[test]
fn missing_and_failed_libraries_report_errors_instead_of_using_bundled_code() {
    for source in [
        "module := {api: 1, id: \"timer\", kind: \"library\", enabled: false}",
        "module := {api: 1, id: \"timer\", kind: \"library\"}\ncreate := fn(kind, args) => error(\"CUSTOM FAILURE\")",
    ] {
        let mut ws = note("watch := stopwatch()\n");
        replace(&mut ws, "timer", source.into());
        assert!(
            RequestContext::new(&ws, now())
                .engine()
                .named(path(), "watch")
                .is_err()
        );
        let html = wtf::rendering::html_in(&RequestContext::new(&ws, now()), path()).unwrap();
        assert!(!html.contains("00:00 elapsed"));
        assert!(
            html.contains("disabled") || html.contains("CUSTOM FAILURE"),
            "{html}"
        );
    }
}
