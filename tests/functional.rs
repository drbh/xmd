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

#[test]
fn multiline_functions_records_and_comments_share_the_note_language() {
    let source = r#"// The closure keeps its defining scope.
rate := 10%
with_tax := fn(
  cost
) => (
  // Only this branch is evaluated.
  if(
    cost > $0,
    cost + cost * rate,
    $0
  )
)
result := fold(
  map([$2, $3], with_tax),
  $0,
  fn(total, price) => total + price
)
config := {
  title: "https://example.com/)", // Delimiters here are data.
  values: [
    1,
    2
  ]
}
add := fn(x) =>
  fn(y) => x + y
"#;
    for source in [source.to_string(), source.replace('\n', "\r\n")] {
        let ws = workspace(&source);
        let doc = &ws.documents[path()];
        assert_eq!(doc.definitions.len(), 5);
        assert!(doc.sections.is_empty() && doc.tasks.is_empty() && doc.links.is_empty());
        let mut e = engine(&ws);
        assert_eq!(e.named(path(), "result").unwrap().display(), "$5.50");
        assert_eq!(e.eval(path(), "add(2)(3)").unwrap(), Value::Number(5.0));
        assert_eq!(
            e.eval(path(), "config.title").unwrap().display(),
            "https://example.com/)"
        );
        assert!(wtf::diagnostics::collect(&ws, path(), e.today, e.now, false).is_empty());
        assert!(
            !doc.references
                .iter()
                .any(|r| ["cost", "total", "price", "x", "y"].contains(&r.name.as_str()))
        );
        let definition = &doc.definitions[1];
        assert_eq!(definition.end.line, 11);
        assert_eq!(definition.value_span.range(&source).end.line, 11);
        let outline = wtf::symbols::document_symbols(&ws, path(), e.now);
        assert_eq!(outline[1].range.end.line, 11);
        assert!(
            wtf::symbols::folding_ranges(doc)
                .iter()
                .any(|f| f.start_line == 2 && f.end_line == 11)
        );
    }
}

#[test]
fn multiline_errors_and_references_keep_exact_source_locations() {
    let source = "rate := 2\nbad := fn(x) => (\n  // A Unicode prefix must not move the error.\n  length(\"🦀\") + x / 0\n)\nresult := (\n  rate + bad(1)\n)\n";
    for source in [source.to_string(), source.replace('\n', "\r\n")] {
        let ws = workspace(&source);
        let mut e = engine(&ws);
        assert!(e.named(path(), "result").is_err());
        let failure = e.failure.take().unwrap();
        assert_eq!(failure.span.line, 3);
        assert_eq!(failure.span.source(&source), "0");
        let line = ws.documents[path()].line(3);
        assert_eq!(
            failure.span.range(&source).start.character as usize,
            line[..line.find('0').unwrap()].encode_utf16().count()
        );
        let reference = ws.documents[path()]
            .references
            .iter()
            .find(|r| r.name == "rate" && r.span.line == 6)
            .unwrap();
        assert_eq!(reference.span.source(&source), "rate");
        assert_eq!(
            wtf::intelligence::symbol_at(&ws, path(), reference.span.range(&source).start)
                .unwrap()
                .0,
            ws.resolve(path(), "rate").unwrap()
        );
        let dependencies =
            wtf::hierarchy::dependencies(&ws, &ws.resolve(path(), "result").unwrap());
        assert!(dependencies.iter().any(|(s, _)| ws.named(s).name == "rate"));
    }
}

#[test]
fn unfinished_multiline_expressions_do_not_consume_the_next_definition_or_prose() {
    for source in [
        "bad := fn(x) => (\n  x +\ngood := 42\nOrdinary prose\n",
        "bad := fn(x) => (\n  x +\n  good := 42\nOrdinary prose\n",
        "bad := fn(x) => (\n  x +\n// A description of good.\ngood := 42\n",
    ] {
        let ws = workspace(source);
        assert_eq!(ws.documents[path()].definitions.len(), 2);
        assert_eq!(
            engine(&ws).named(path(), "good").unwrap(),
            Value::Number(42.0)
        );
        assert!(engine(&ws).named(path(), "bad").is_err());
    }
}

#[test]
fn multiline_modules_compile_and_apply_the_same_limits() {
    let source = "// Summarize headings.\nplugin := {\n  api: 1,\n  id: \"headings\",\n  kind: \"inlay\",\n  inputs: [\"sections\"]\n}\n// Use the selected section anchors.\ncollect := fn(ctx) => (\n  map(\n    ctx.document.sections,\n    fn(s) => {at: s.anchor, label: s.title}\n  )\n)\n";
    let mut ws = workspace("# Hello\n");
    ws.plugins = std::sync::Arc::new(
        wtf::plugins::Plugins::compile(
            [("/notes/.wtf/plugins/headings.wtf".into(), source.into())].into(),
        )
        .unwrap(),
    );
    let result = wtf::presentation::hints_at(
        &ws,
        path(),
        engine(&ws).now,
        lsp_types::Range::new(
            lsp_types::Position::new(0, 0),
            lsp_types::Position::new(2, 0),
        ),
    );
    assert!(serde_json::to_string(&result).unwrap().contains("Hello"));
    let ws = workspace("loop := fn(x) => (\n  loop(x)\n)\n");
    assert!(
        engine(&ws)
            .eval(path(), "loop(1)")
            .unwrap_err()
            .contains("depth")
    );
}

#[test]
fn query_data_operations_are_available_in_note_functions() {
    let ws = workspace("money := $7\nname := \"Example\"\n");
    let mut e = engine(&ws);
    assert_eq!(
        e.eval(path(), "{name, cost:money.amount}.name").unwrap(),
        Value::Text("Example".into())
    );
    assert_eq!(
        e.eval(path(), "sum([{x:30m}, {x:null}, {x:90s}].x)")
            .unwrap(),
        Value::Duration(1890)
    );
    assert_eq!(
        e.eval(path(), "map(sort_by([3, null, 1], fn(x) => x), fn(x) => x)")
            .unwrap(),
        Value::List(vec![Value::Number(1.0), Value::Number(3.0), Value::Null])
    );
    assert_eq!(
        e.eval(path(), "length(group_by([1, 2, 1], fn(x) => x))")
            .unwrap(),
        Value::Count(2)
    );
    assert_eq!(
        e.eval(path(), "null < today()").unwrap(),
        Value::Bool(false)
    );
    assert_eq!(e.eval(path(), "\"a\" < \"b\"").unwrap(), Value::Bool(true));
    assert_eq!(e.eval(path(), "date(now())").unwrap(), Value::Date(e.today));
    assert!(e.eval(path(), "sort_by([1, \"two\"], fn(x) => x)").is_err());
}

#[test]
fn dynamic_evaluation_is_bounded_and_row_function_bindings_capture_lexically() {
    let ws = workspace("code := \"eval(code)\"\n");
    let mut e = engine(&ws);
    assert!(e.eval(path(), "eval(code)").unwrap_err().contains("depth"));
    assert_eq!(e.eval(path(), "1 + 2").unwrap(), Value::Number(3.0));
    let query =
        wtf::query::Query::parse("[{f:fn(x) => x + 1}] | select map([1], fn(x) => f(x))").unwrap();
    assert_eq!(
        serde_json::json!(
            wtf::query::execute(&ws, &query, &wtf::query::QueryContext::new(e.now))
                .unwrap()
                .rows
        ),
        serde_json::json!([[2.0]])
    );
}

#[test]
fn unfinished_builtin_calls_do_not_become_unknown_function_references() {
    let ws = workspace("broken := sum([1,\nnext := 2\n");
    let e = engine(&ws);
    let issues = wtf::diagnostics::collect(&ws, path(), e.today, e.now, false);
    assert!(!issues.is_empty());
    assert!(
        issues
            .iter()
            .all(|d| !d.message.contains("Unknown name 'sum'"))
    );
}
