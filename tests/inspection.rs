use chrono::DateTime;
use serde_json::{Value, json};
use std::path::Path;
use wtf::{
    RequestContext,
    document::Document,
    query::{self, Query},
    workspace::Workspace,
};

fn workspace(notes: &[(&str, &str)]) -> Workspace {
    Workspace {
        roots: vec!["/notes".into()],
        documents: notes
            .iter()
            .map(|(name, source)| {
                (
                    Path::new("/notes").join(name),
                    Document::parse((*source).into()),
                )
            })
            .collect(),
        cache: Default::default(),
        lookups: Default::default(),
        modules: Default::default(),
    }
}
fn run(ws: &Workspace, source: &str, file: Option<&str>) -> Value {
    let request = RequestContext::new(
        ws,
        DateTime::parse_from_rfc3339("2026-09-18T12:00:00Z").unwrap(),
    );
    json!(
        query::execute_scoped_in(
            &request,
            &Query::parse(source).unwrap(),
            file.map(Path::new)
        )
        .unwrap()
        .rows
    )
}
fn offset(text: &str, position: &Value) -> usize {
    let line = position["line"].as_u64().unwrap() as usize;
    let prefix: usize = text.split_inclusive('\n').take(line).map(str::len).sum();
    prefix
        + wtf::document::byte_at(
            text[prefix..].split('\n').next().unwrap(),
            position["character"].as_u64().unwrap() as u32,
        )
        .unwrap()
}

#[test]
fn syntax_is_queryable_without_evaluation_and_retains_exact_source_and_relationships() {
    let source = "# Notes 🦀\r\nPlain prose.\r\n[add] := fn(x) => (\r\n  // keep this comment\r\n  x + missing\r\n)\r\n- [ ] Parent\r\n  - [x] Child @estimate(30m)\r\nSee [add].\r\n```\r\ninert := 5\r\n```\r\n";
    let ws = workspace(&[("one.wtf", source), ("two.wtf", "99:other\n")]);
    let nodes = run(&ws, "ast", Some("/notes/one.wtf"));
    let nodes = nodes.as_array().unwrap();
    assert_eq!(nodes[0]["kind"], "document");
    assert_eq!(nodes[0]["text"], source);
    let lines: String = nodes
        .iter()
        .filter(|n| n["kind"] == "line")
        .map(|n| n["text"].as_str().unwrap())
        .collect();
    assert_eq!(lines, source);
    for node in nodes {
        let range = &node["source"]["range"];
        assert_eq!(
            &source[offset(source, &range["start"])..offset(source, &range["end"])],
            node["text"].as_str().unwrap(),
            "{node}"
        );
        for child in node["children"].as_array().unwrap() {
            assert_eq!(
                nodes.iter().find(|n| &n["id"] == child).unwrap()["parent"],
                node["id"]
            );
        }
        if !node["parent"].is_null() {
            assert!(
                nodes.iter().find(|n| n["id"] == node["parent"]).unwrap()["children"]
                    .as_array()
                    .unwrap()
                    .contains(&node["id"])
            );
        }
    }
    let defs = run(
        &ws,
        "map(filter(ast, fn(n) => n.kind == \"definition\"), fn(n) => {name:n.name, text:n.text})",
        Some("/notes/one.wtf"),
    );
    assert_eq!(defs.as_array().unwrap().len(), 1);
    assert_eq!(defs[0]["name"], "add");
    assert!(
        defs[0]["text"]
            .as_str()
            .unwrap()
            .starts_with("[add] := fn(x)")
    );
    assert_eq!(
        run(
            &ws,
            "ast | where kind == \"lambda\" | select parameters",
            Some("/notes/one.wtf")
        ),
        json!([["x"]])
    );
    assert_eq!(
        run(
            &ws,
            "ast | where kind == \"name\" && name == \"missing\" | select text",
            Some("/notes/one.wtf")
        ),
        json!(["missing"])
    );
    assert_eq!(
        run(
            &ws,
            "ast | where kind == \"checkbox\" | select text",
            Some("/notes/one.wtf")
        ),
        json!(["[ ]", "[x]"])
    );
}

#[test]
fn syntax_covers_tables_plans_itineraries_links_and_broken_expressions() {
    let ws = workspace(&[
        ("tables.wtf", include_str!("../examples/12-tables.wtf")),
        ("plans.wtf", include_str!("../examples/14-plans.wtf")),
        (
            "other.wtf",
            "Wednesday, September 16, 2026\n10:00 AM + Museum\n  Address: Main street\nhttps://example.com\nbroken := 1 +\n",
        ),
    ]);
    let kinds = run(&ws, "map(ast, fn(n) => n.kind)", None);
    for kind in [
        "table",
        "column",
        "row",
        "cell",
        "plan",
        "objective",
        "constraint",
        "day",
        "stop",
        "detail",
        "link",
        "parse_error",
    ] {
        assert!(
            kinds.as_array().unwrap().contains(&json!(kind)),
            "missing {kind}"
        );
    }
    assert_eq!(
        run(
            &ws,
            "filter(ast, fn(n) => n.kind == \"parse_error\") | select source.path",
            None
        ),
        json!(["/notes/other.wtf"])
    );
    let empty = workspace(&[("empty.wtf", "")]);
    assert_eq!(
        run(&empty, "ast | select {kind, text}", None),
        json!([{"kind":"document","text":""}])
    );
}

#[test]
fn graph_shares_editor_edges_including_functions_tables_cycles_and_external_endpoints() {
    let ws = workspace(&[
        (
            "one.wtf",
            "double := fn(x) => x * import(\"./two.wtf\").rate\nanswer := double(2)\nloop := loop + 1\nt := table\n| value |\n|---|\n| [import(\"./two.wtf\").rate] |\n# Work :work\n- [ ] Parent\n  - [ ] Child\n",
        ),
        ("two.wtf", "3:rate\nextra := rate + 2\n"),
    ]);
    let result = run(&ws, "graph", Some("/notes/one.wtf"));
    let graph = &result[0];
    let nodes = graph["nodes"].as_array().unwrap();
    let node = |name: &str| nodes.iter().find(|n| n["name"] == name).unwrap();
    assert_eq!(node("rate")["external"], true);
    assert!(!nodes.iter().any(|n| n["name"] == "extra"));
    for (from, to) in [
        ("answer", "double"),
        ("double", "rate"),
        ("loop", "loop"),
        ("t", "rate"),
        ("Parent", "Child"),
        ("work", "Child"),
    ] {
        assert!(
            graph["edges"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["from"] == node(from)["id"] && e["to"] == node(to)["id"]),
            "missing {from} -> {to}"
        );
    }
    for edge in graph["edges"].as_array().unwrap() {
        assert!(nodes.iter().any(|n| n["id"] == edge["from"]));
        assert!(nodes.iter().any(|n| n["id"] == edge["to"]));
        assert_eq!(edge["reads"][0]["path"], "/notes/one.wtf");
    }
    assert_eq!(
        run(
            &ws,
            "graph.nodes | where external | select name",
            Some("/notes/one.wtf")
        ),
        json!(["rate"])
    );
    assert_eq!(
        run(
            &ws,
            "length(filter(graph.nodes, fn(n) => n.external))",
            None
        ),
        json!([0])
    );
}
