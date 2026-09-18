use chrono::DateTime;
use serde_json::{Value, json};
use wtf::{
    document::Document,
    query::{self, Query, QueryContext},
    workspace::Workspace,
};

fn workspace(notes: &[(&str, &str)]) -> Workspace {
    Workspace {
        roots: vec!["/notes".into()],
        documents: notes
            .iter()
            .map(|(p, s)| {
                (
                    std::path::Path::new("/notes").join(p),
                    Document::parse((*s).into()),
                )
            })
            .collect(),
        cache: Default::default(),
        lookups: Default::default(),
        modules: Default::default(),
    }
}
fn context() -> QueryContext {
    QueryContext::new(DateTime::parse_from_rfc3339("2026-09-16T12:00:00-04:00").unwrap())
}
fn run(ws: &Workspace, source: &str) -> Value {
    json!(
        query::execute(ws, &Query::parse(source).unwrap(), &context())
            .unwrap()
            .rows
    )
}

#[test]
fn task_queries_preserve_hierarchy_source_units_and_composition() {
    let ws = workspace(&[(
        "tasks.wtf",
        "- [ ] Parent\n  - [ ] Child #errands @due(2026-09-17) @estimate(90s)\n  - [x] Done\n- [ ] Later @due(2026-10-01)\n",
    )]);
    assert_eq!(run(&ws, "tasks | count"), json!([4]));
    let result = run(
        &ws,
        "tasks | where leaf && !done && contains(tags, \"errands\") | select {title, due, estimate, source, parent} | sort due | limit 1",
    );
    assert_eq!(result[0]["title"], "Child #errands");
    assert_eq!(
        result[0]["due"],
        json!({"type":"date","value":"2026-09-17"})
    );
    assert_eq!(
        result[0]["estimate"],
        json!({"type":"duration","seconds":90})
    );
    assert_eq!(result[0]["source"]["line"], 2);
    assert_eq!(result[0]["source"]["range"]["start"]["line"], 1);
    assert_eq!(result[0]["parent"]["line"], 1);
    assert_eq!(run(&ws, "tasks | where parent != null | count"), json!([2]));
}

#[test]
fn pipeline_parsing_handles_strings_boolean_pipes_and_nested_expressions() {
    let ws = workspace(&[("n.wtf", "- [ ] a|b\n- [ ] c\n")]);
    assert_eq!(
        run(
            &ws,
            r#"tasks | where title == "a|b" || (false && nonexistent) | select {label: title, separator: ":,|", nested: contains(title, "|"), timestamp: 2026-09-16T12:00:00-04:00}"#
        )[0],
        json!({"label":"a|b","separator":":,|","nested":true,"timestamp":{"type":"datetime","value":"2026-09-16T12:00:00-04:00"}})
    );
    assert_eq!(
        run(&ws, "tasks | where true || nonexistent | count"),
        json!([2])
    );
    assert_eq!(
        run(
            &ws,
            "tasks | select {name: title} | where contains(name, \"b\") | select name"
        ),
        json!(["a|b"])
    );
}

#[test]
fn one_clock_and_offset_control_queries_and_calculated_values() {
    let ws = workspace(&[(
        "time.wtf",
        "day := today()\nclock := now()\n- Late @at(2026-09-17T01:00:00+00:00)\n",
    )]);
    let ctx = QueryContext::new(DateTime::parse_from_rfc3339("2026-09-16T23:00:00-04:00").unwrap());
    let result = json!(
        query::execute(
            &ws,
            &Query::parse("values | select {name, value}").unwrap(),
            &ctx
        )
        .unwrap()
        .rows
    );
    assert_eq!(
        result[0]["value"],
        json!({"type":"date","value":"2026-09-16"})
    );
    assert_eq!(
        result[1]["value"],
        json!({"type":"datetime","value":"2026-09-16T23:00:00-04:00"})
    );
    let events = json!(
        query::execute(
            &ws,
            &Query::parse("events | where at_date == today()").unwrap(),
            &ctx
        )
        .unwrap()
        .rows
    );
    assert_eq!(events.as_array().unwrap().len(), 1);
    assert_eq!(
        run(
            &ws,
            "values | where name == \"day\" | select {tomorrow: value + 1d}"
        )[0]["tomorrow"]["value"],
        "2026-09-17"
    );
}

#[test]
fn sorting_is_stable_with_nulls_last_in_both_directions() {
    let ws = workspace(&[(
        "n.wtf",
        "- [ ] Missing\n- [ ] B @due(2026-09-17)\n- [ ] A @due(2026-09-16)\n- [ ] C @due(2026-09-17)\n",
    )]);
    assert_eq!(
        run(&ws, "tasks | sort due | select title"),
        json!(["A", "B", "C", "Missing"])
    );
    assert_eq!(
        run(&ws, "tasks | sort due desc, title desc | select title"),
        json!(["C", "B", "A", "Missing"])
    );
    assert_eq!(run(&ws, "tasks | where due < today() | count"), json!([0]));
}

#[test]
fn typed_aggregation_grouping_and_nested_projection() {
    let ws = workspace(&[
        ("a.wtf", "- [ ] A @estimate(30m)\n- [ ] B @estimate(90s)\n"),
        ("b.wtf", "- [ ] C\n"),
    ]);
    assert_eq!(
        run(&ws, "tasks | sum estimate"),
        json!([{"type":"duration","seconds":1890}])
    );
    let grouped = run(
        &ws,
        "tasks | group source.path | select {path:key, tasks:length(rows), effort:sum(rows.estimate)} | sort path",
    );
    assert_eq!(
        grouped,
        json!([
            {"path":"/notes/a.wtf","tasks":2,"effort":{"type":"duration","seconds":1890}},
            {"path":"/notes/b.wtf","tasks":1,"effort":null}
        ])
    );
    assert_eq!(run(&workspace(&[]), "tasks | sum estimate"), json!([null]));
}

#[test]
fn definitions_resolve_in_their_own_document_and_keep_money_typed() {
    let ws = workspace(&[
        ("a.wtf", "$5:price\ntotal := price * 2\n"),
        ("b.wtf", "€7:price\ntotal := price * 3\n"),
    ]);
    let result = run(
        &ws,
        "values | where name == \"total\" | select {value, amount:value.amount, currency:value.currency, local:eval(\"price\")}",
    );
    assert_eq!(
        result[0],
        json!({"value":{"type":"money","amount":10.0,"currency":"USD"},"amount":10.0,"currency":"USD","local":{"type":"money","amount":5.0,"currency":"USD"}})
    );
    assert_eq!(result[1]["currency"], "EUR");
    let err = query::execute(
        &ws,
        &Query::parse("values | where name == \"price\" | sum value").unwrap(),
        &context(),
    )
    .unwrap_err();
    assert!(err.contains("currency") || err.contains("USD"), "{err}");
}

#[test]
fn table_rows_and_plan_solutions_are_structured_values() {
    let ws = workspace(&[
        ("tables.wtf", include_str!("../examples/12-tables.wtf")),
        ("plans.wtf", include_str!("../examples/14-plans.wtf")),
    ]);
    let rows = run(
        &ws,
        "rows | where table == \"groceries\" && cells.quantity > 2 | select {item:cells.item, cost:cells.price * cells.quantity}",
    );
    assert_eq!(
        rows,
        json!([{"item":"pear","cost":{"type":"money","amount":17.2,"currency":"USD"}}])
    );
    let plan = run(&ws, "plans | where name == \"bakery\" | select solution");
    assert_eq!(plan[0]["goal"], "maximize");
    assert_eq!(plan[0]["objective"]["type"], "money");
    assert!(plan[0]["variables"]["bagels"].as_f64().unwrap() >= 12.0);
    assert_eq!(plan[0]["constraints"][0]["name"], "flour");
    assert_eq!(
        run(&ws, "tables | select {name}"),
        json!([{"name":"groceries"}])
    );
}

#[test]
fn evaluation_errors_are_distinct_from_missing_values_and_diagnostics_are_queryable() {
    let ws = workspace(&[(
        "bad.wtf",
        "bad := missing + 1\n- [ ] Broken @due(missing)\n- [ ] Undated\n",
    )]);
    assert_eq!(run(&ws, "values | select {name}"), json!([{"name":"bad"}]));
    let values = run(&ws, "values");
    assert!(values[0]["value"].is_null());
    assert!(values[0]["errors"][0].as_str().unwrap().contains("missing"));
    let tasks = run(&ws, "tasks | select {due, errors}");
    assert!(tasks[0]["due"].is_null() && tasks[1]["due"].is_null());
    assert_eq!(tasks[0]["errors"].as_array().unwrap().len(), 1);
    assert_eq!(tasks[1]["errors"], json!([]));
    assert!(
        run(&ws, "diagnostics | where severity == \"error\"")
            .as_array()
            .unwrap()
            .len()
            >= 2
    );
}

#[test]
fn unions_and_cached_resources_work_without_io() {
    let mut ws = workspace(&[(
        "n.wtf",
        "- [ ] Task\n- Event @at(2026-09-16T15:00:00-04:00)\nhttps://github.com/o/r/pull/1\n",
    )]);
    ws.cache.insert(
        "https://github.com/o/r/pull/1".into(),
        wtf::resources::Metadata {
            title: "Cached".into(),
            state: "OPEN".into(),
            merged: Some(false),
            checks: None,
            review: None,
            fetched_at: context().now.into(),
            provider: None,
            data: None,
        },
    );
    assert_eq!(
        run(&ws, "union(tasks, events) | select kind"),
        json!(["task", "event"])
    );
    assert_eq!(
        run(
            &ws,
            "resources | where metadata != null | select metadata.title"
        ),
        json!(["Cached"])
    );
}

#[test]
fn malformed_queries_and_incompatible_values_return_errors() {
    for source in [
        "",
        "tasks |",
        "tasks | where (true",
        "tasks | select {a:1,a:2}",
        "tasks | limit -1",
        "tasks | explode",
        "tasks | select {x:}",
    ] {
        assert!(Query::parse(source).is_err(), "{source}");
    }
    let ws = workspace(&[("n.wtf", "- [ ] A\n")]);
    for source in [
        "tasks | where title",
        "tasks | where dne",
        "tasks | select title.dne",
        "tasks | select bogus()",
        "tasks | sort tags",
        "tasks | select {x: 1 + 2d}",
    ] {
        assert!(
            query::execute(&ws, &Query::parse(source).unwrap(), &context()).is_err(),
            "{source}"
        );
    }
    assert!(
        Query::parse(&format!(
            "tasks | where {}true{}",
            "(".repeat(65),
            ")".repeat(65)
        ))
        .is_err()
    );
    assert!(Query::parse(&format!("tasks | select {}", "1+".repeat(1000))).is_err());
}

#[test]
fn saved_agendas_preserve_task_event_and_itinerary_semantics() {
    let ws = workspace(&[(
        "n.wtf",
        concat!(
            "- [ ] Parent\n  - [ ] Undated\n  - [x] Done\n",
            "- [ ] Overdue @due(2026-09-15)\n- [ ] Today @due(2026-09-16)\n",
            "- [ ] Next @scheduled(2026-09-18)\n- [ ] Future @due(2026-10-01)\n",
            "- [ ] Recurring @every(day)\n- [ ] Broken @due(missing)\n",
            "- Morning @at(2026-09-16T09:00:00-04:00)\n- Afternoon @at(2026-09-16T16:00:00-04:00)\n",
            "- Tomorrow @at(2026-09-17T09:00:00-04:00)\n- Yesterday @at(2026-09-15T09:00:00-04:00)\n",
            "- Broken event @at(missing)\nWednesday, September 16, 2026\n10:00 AM + Museum\n"
        ),
    )]);
    let today = json!([
        "Overdue",
        "Undated",
        "Today",
        "Recurring",
        "Broken",
        "Broken event",
        "Morning",
        "+ Museum",
        "Afternoon"
    ]);
    assert_eq!(run(&ws, "@today | select title"), today);
    let mut week = today.as_array().unwrap().clone();
    week.extend([json!("Tomorrow"), json!("Next")]);
    assert_eq!(run(&ws, "@week | select title"), json!(week));
    assert_eq!(run(&ws, "@today | count"), json!([9]));
}

#[test]
fn mixed_sort_types_and_long_flat_expressions_fail_without_panics() {
    let ws = workspace(&[("n.wtf", "1:a\n\"text\":b\n2:c\n")]);
    let error = query::execute(
        &ws,
        &Query::parse("values | sort value").unwrap(),
        &context(),
    )
    .unwrap_err();
    assert!(error.contains("compare"), "{error}");
    let long = format!("tasks | select {}", vec!["1"; 100].join(" + "));
    assert!(Query::parse(&long).unwrap_err().contains("depth"));
    assert!(Query::parse("@unknown").is_err());
}

#[test]
fn functional_queries_share_the_note_language_and_can_scope_a_document() {
    let ws = workspace(&[
        (
            "a.wtf",
            "double := fn(x) => x * 2\n1:n\n- [ ] First @estimate(30m)\n- [x] Done\n",
        ),
        ("b.wtf", "9:n\n- [ ] Other @estimate(1h)\n"),
    ]);
    let request = wtf::RequestContext::new(&ws, context().now);
    let path = std::path::Path::new("/notes/a.wtf");
    let run_file = |source| {
        json!(
            query::execute_scoped_in(&request, &Query::parse(source).unwrap(), Some(path))
                .unwrap()
                .rows
        )
    };
    assert_eq!(
        run_file("map(filter(tasks, fn(t) => !t.done), fn(t) => t.title)"),
        json!(["First"])
    );
    assert_eq!(
        run_file("tasks | where !done | select title"),
        json!(["First"])
    );
    assert_eq!(
        run_file("{count: length(tasks), value: double(n)}"),
        json!([{"count":2,"value":2.0}])
    );
    assert_eq!(
        run_file("sum(tasks.estimate)"),
        json!([{"type":"duration","seconds":1800}])
    );
    assert_eq!(run(&ws, "length(tasks)"), json!([3]));
    assert_eq!(
        run_file("map(sort_by(tasks, fn(t) => t.title), fn(t) => upper(t.title))"),
        json!(["DONE", "FIRST"])
    );
    assert_eq!(
        run_file(
            "map(group_by(tasks, fn(t) => t.done), fn(g) => {done:g.key, count:length(g.rows)})"
        ),
        json!([{"done":false,"count":1},{"done":true,"count":1}])
    );
    assert!(
        query::execute_scoped_in(
            &request,
            &Query::parse("tasks").unwrap(),
            Some(std::path::Path::new("/notes/missing.wtf"))
        )
        .unwrap_err()
        .contains("not indexed")
    );
}

#[test]
fn shared_query_scopes_capture_lexically_without_leaking_into_definitions() {
    let ws = workspace(&[("a.wtf", "value := 7\nread := fn() => value\n- [ ] A\n")]);
    assert_eq!(
        run(
            &ws,
            "tasks | select {value: 100, title} | select {local: map([1], fn(x) => x + value), named: read()}"
        ),
        json!([{"local":[101.0],"named":7.0}])
    );
    assert_eq!(
        run(&ws, "tasks | select map([1], fn(title) => title + 1)"),
        json!([[2.0]])
    );
    assert_eq!(
        run(&ws, "tasks | select if(false, nonexistent, title)"),
        json!(["A"])
    );
    assert_eq!(run(&ws, "if(true, 1, values)"), json!([1.0]));
    assert_eq!(
        run(
            &ws,
            "tasks | select { // | , } inside a comment\n title, nested: {title}}"
        ),
        json!([{"title":"A", "nested":{"title":"A"}}])
    );
    assert!(query::execute(&ws, &Query::parse("unknown").unwrap(), &context()).is_err());
}
