use chrono::{DateTime, FixedOffset};
use std::{collections::BTreeMap, path::Path};
use wtf::{
    document::Document,
    engine::{Currency, Engine, Value},
    highlighting::{TOKEN_TYPES, semantic_tokens},
    intelligence,
    workspace::Workspace,
};

fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-09-16T14:00:00-04:00").unwrap()
}
fn path() -> &'static Path {
    Path::new("/notes/test.wtf")
}
fn note(source: &str) -> Workspace {
    Workspace {
        roots: vec!["/notes".into()],
        documents: [(path().to_path_buf(), Document::parse(source.into()))].into(),
        cache: BTreeMap::new(),
        lookups: Default::default(),
        modules: Default::default(),
    }
}
fn eval(ws: &Workspace, name: &str) -> Result<Value, String> {
    Engine::at(ws, now()).named(path(), name)
}

#[test]
fn scalars_quoted_text_and_resources_define_without_brackets() {
    let ws = note(
        "Our budget is $3,000:budget and we've spent $2,444:spent.\nDeparture 2026-11-20:departure, \"Oaxaca City\":city, 47%:share, 2h:slot, true:flag, 12:count.\nPR https://github.com/zed-industries/zed/pull/1:zed_pr and geo:17.06,-96.72:zocalo (€450:hotel).\n",
    );
    let names: Vec<_> = ws.documents[path()]
        .definitions
        .iter()
        .map(|d| d.named.name.as_str())
        .collect();
    assert_eq!(
        names,
        [
            "budget",
            "spent",
            "departure",
            "city",
            "share",
            "slot",
            "flag",
            "count",
            "zed_pr",
            "zocalo",
            "hotel"
        ]
    );
    assert_eq!(
        eval(&ws, "budget").unwrap(),
        Value::Money(3000.0, Currency::USD)
    );
    assert_eq!(
        eval(&ws, "city").unwrap(),
        Value::Text("Oaxaca City".into())
    );
    assert_eq!(eval(&ws, "share").unwrap(), Value::Ratio(0.47));
    assert_eq!(eval(&ws, "slot").unwrap(), Value::Duration(7200));
    assert_eq!(eval(&ws, "flag").unwrap(), Value::Bool(true));
    assert_eq!(eval(&ws, "hotel").unwrap().display(), "€450");
    assert!(
        matches!(eval(&ws, "zed_pr").unwrap(), Value::Resource(r) if r.target.ends_with("/pull/1"))
    );
    assert!(
        matches!(eval(&ws, "zocalo").unwrap(), Value::Resource(r) if r.target == "geo:17.06,-96.72")
    );
    assert_eq!(
        wtf::RequestContext::new(&ws, now())
            .diagnostics(path(), false)
            .len(),
        0
    );
    // Spans: the value is highlighted as a literal, the name as a declaration.
    let doc = &ws.documents[path()];
    let budget = &doc.definitions[0];
    assert_eq!(
        &doc.line(0)[budget.value_span.start..budget.value_span.end],
        "$3,000"
    );
    assert_eq!(
        &doc.line(0)[budget.named.span.start..budget.named.span.end],
        "budget"
    );
    let tokens = semantic_tokens(doc);
    let kinds: Vec<&str> = tokens
        .iter()
        .map(|t| TOKEN_TYPES[t.token_type as usize])
        .collect();
    assert!(
        kinds.contains(&"wtfMoney") && kinds.contains(&"variable") && kinds.contains(&"wtfLink")
    );
    // Bare resources are links too, so the Open lens and hover work.
    assert!(doc.links.iter().any(|l| l.target.contains("/pull/1")));
}

#[test]
fn calculations_define_without_brackets_and_prose_is_left_alone() {
    let ws = note(
        "$3,000:budget\n$2,444:spent\nremaining := budget - spent\nshare := remaining / budget\nWe have [remaining] left ([share]).\n",
    );
    assert_eq!(eval(&ws, "remaining").unwrap().display(), "$556");
    assert_eq!(eval(&ws, "share").unwrap().display(), "18.5333%");
    assert_eq!(
        wtf::RequestContext::new(&ws, now())
            .diagnostics(path(), false)
            .len(),
        0
    );
    let hover = wtf::RequestContext::new(&ws, now())
        .symbol_hover(&ws.resolve(path(), "remaining").unwrap());
    assert!(
        hover.contains("budget - spent\n= $3,000 - $2,444\n= $556"),
        "{hover}"
    );
    // Times, ratios, words, escaped colons and true/false are not definitions.
    let prose = note(
        "Meet at 10:30am, ratio 3:1, see note:budget, the year 2026\\:plan, and 1:true.\nNote: something\n- [ ] Pay :deposit\n## Launch :launch\n",
    );
    let doc = &prose.documents[path()];
    assert!(
        doc.definitions.is_empty(),
        "{:?}",
        doc.definitions
            .iter()
            .map(|d| &d.named.name)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        doc.tasks[0].named.as_ref().map(|n| n.name.as_str()),
        Some("deposit")
    );
    assert_eq!(
        doc.sections[0].named.as_ref().map(|n| n.name.as_str()),
        Some("launch")
    );
    // Both spellings coexist and rename touches every occurrence.
    let both = note("[$3,000]:budget and $2,444:spent\ncash := budget - spent\n");
    let spent = both.resolve(path(), "spent").unwrap();
    let uses = intelligence::occurrences(&both, &spent);
    assert_eq!(uses.len(), 2);
    assert_eq!(eval(&both, "cash").unwrap().display(), "$556");
}
