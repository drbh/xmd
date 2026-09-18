use chrono::DateTime;
use std::path::Path;
use wtf::{
    document::Document,
    engine::{Engine, Value},
    workspace::Workspace,
};

fn workspace(source: &str) -> Workspace {
    Workspace {
        roots: vec!["/notes".into()],
        documents: [("/notes/test.wtf".into(), Document::parse(source.into()))].into(),
        cache: Default::default(),
        lookups: Default::default(),
        plugins: Default::default(),
    }
}
fn engine(ws: &Workspace) -> Engine<'_> {
    Engine::at(
        ws,
        DateTime::parse_from_rfc3339("2026-09-18T12:00:00Z").unwrap(),
    )
}
fn path() -> &'static Path {
    Path::new("/notes/test.wtf")
}

#[test]
fn functions_capture_lexical_scopes_and_keep_units() {
    let ws = workspace(
        "rate := 10%\nadd := fn(x) => fn(y) => x + y\nadd_tax := fn(cost) => cost + cost * rate\ncaller := fn(rate) => add_tax($100)\n",
    );
    let mut e = engine(&ws);
    assert_eq!(e.eval(path(), "add(2)(3)").unwrap(), Value::Number(5.0));
    assert_eq!(e.eval(path(), "caller(90%)").unwrap().display(), "$110");
    assert_eq!(
        e.eval(path(), "map([$2, $3], add_tax)").unwrap().display(),
        "[$2.2, $3.3]"
    );
    assert!(wtf::diagnostics::collect(&ws, path(), e.today, e.now, false).is_empty());
}

#[test]
fn records_lists_and_lazy_branches_work_in_notes_and_queries() {
    let ws = workspace(
        "double := fn(x) => x * 2\nresult := fold(map(filter([1, 2, 3], fn(x) => x > 1), double), 0, fn(a, b) => a + b)\n",
    );
    let mut e = engine(&ws);
    assert_eq!(e.named(path(), "result").unwrap(), Value::Number(10.0));
    assert_eq!(
        e.eval(path(), "if(true, {title: upper(\"hello\")}, 1 / 0).title")
            .unwrap(),
        Value::Text("HELLO".into())
    );
    assert_eq!(
        e.eval(path(), "coalesce(get({}, \"missing\"), get([3, 4], 1))")
            .unwrap(),
        Value::Number(4.0)
    );
    let query = wtf::query::Query::parse(
        "values | where name == \"result\" | select map([1, 2], fn(x) => x + value)",
    )
    .unwrap();
    let result = wtf::query::execute(&ws, &query, &wtf::query::QueryContext::new(e.now)).unwrap();
    assert_eq!(
        serde_json::to_value(result).unwrap()["rows"],
        serde_json::json!([[11.0, 12.0]])
    );
}

#[test]
fn function_failures_restore_scope_and_limits_stop_recursion() {
    let ws =
        workspace("loop := fn(x) => loop(x)\nbad := fn(x) => missing\ngood := fn(x) => x + 1\n");
    let mut e = engine(&ws);
    assert!(e.eval(path(), "loop(1)").unwrap_err().contains("depth"));
    assert!(e.eval(path(), "bad(2)").is_err());
    assert_eq!(e.eval(path(), "good(2)").unwrap(), Value::Number(3.0));
    for source in [
        "fn(x, x) => x",
        "{x: 1, x: 2}",
        "map([1], fn(x) => x)(2)",
        "if(1, 2, 3)",
        "good()",
    ] {
        assert!(e.eval(path(), source).is_err(), "{source}");
    }
}

#[test]
fn errors_point_into_the_function_and_collection_growth_is_bounded() {
    let ws = workspace("bad := fn(x) => x / 0\ngrow := fn(x) => [x, x]\n");
    let mut e = engine(&ws);
    assert!(e.eval(path(), "bad(1)").is_err());
    let failure = e.failure.take().unwrap();
    assert_eq!(failure.path, path());
    assert_eq!(failure.span.line, 0);
    assert_eq!(
        &ws.documents[path()].line(0)[failure.span.start..failure.span.end],
        "0"
    );
    let error = e
        .eval(
            path(),
            "fold([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0], 0, fn(a, x) => grow(a))",
        )
        .unwrap_err();
    assert!(error.contains("size limit"), "{error}");
    assert!(e.eval(path(),"join([\"a\", \"b\", \"c\"], replace(\"xxxxxxxxxx\", \"x\", replace(\"xxxxxxxxxx\", \"x\", \"small\")))").is_ok());
}

#[test]
fn existing_builtins_keep_their_meaning_when_notes_use_the_same_name() {
    let ws = workspace("now := 1\nread_clock := fn() => now()\n");
    let mut e = engine(&ws);
    assert_eq!(
        e.eval(path(), "read_clock()").unwrap(),
        Value::DateTime(e.now)
    );
    assert_eq!(e.named(path(), "now").unwrap(), Value::Number(1.0));
}

#[test]
fn presentation_primitives_preserve_unicode_units_and_limits() {
    let ws = workspace("");
    let mut e = engine(&ws);
    for (expression, expected) in [
        ("slice(\"a🦀z\", 1, 2)", "🦀"),
        ("join(concat([\"a\"], [\"b\"]), \"/\")", "a/b"),
        ("repeat(\"█\", round(2.6))", "███"),
        ("format_date(today(), \"%Y-%m-%d\")", "2026-09-18"),
        ("trim(\"  hi  \")", "hi"),
    ] {
        assert_eq!(e.eval(path(), expression).unwrap().display(), expected);
    }
    for expression in [
        "repeat(\"a\", 1000000000)",
        "slice([], 2, 1)",
        "format_date(today(), \"%H\")",
        "format_date(today(), \"%Q\")",
        "error(\"bad\")",
    ] {
        assert!(e.eval(path(), expression).is_err(), "{expression}");
    }
}
