use chrono::{DateTime, FixedOffset};
use lsp_types::{Position, Range, Url};
use std::{
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};
use wtf::{
    RequestContext,
    document::Document,
    engine::Value,
    link_features::{LinkContext, LinkFeature, LinkFeatures},
    workspace::Workspace,
};

fn path() -> &'static Path {
    Path::new("/notes/a.wtf")
}
fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-09-16T12:00:00-04:00").unwrap()
}
fn workspace(source: &str) -> Workspace {
    Workspace {
        roots: vec!["/notes".into()],
        documents: [(path().into(), Document::parse(source.into()))].into(),
        cache: Default::default(),
        lookups: Default::default(),
    }
}
fn all() -> Range {
    Range::new(Position::new(0, 0), Position::new(u32::MAX, 0))
}
struct Counter(AtomicUsize);
impl LinkFeature for Counter {
    fn matches(&self, url: &Url) -> bool {
        url.host_str() == Some("count.example")
    }
    fn inlay(&self, _: &LinkContext<'_>) -> String {
        "counter".into()
    }
    fn property_names(&self, _: &Url) -> &'static [&'static str] {
        &["answer"]
    }
    fn property(&self, _: &LinkContext<'_>, name: &str) -> Result<Value, String> {
        assert_eq!(name, "answer");
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(Value::Number(42.0))
    }
}
#[test]
fn features_share_values_and_provider_configuration_only_within_a_request() {
    let mut ws =
        workspace("[https://count.example/1]:link\n[answer] := link.answer\nAnswer [answer].\n");
    ws.documents.insert(
        "/notes/complete.wtf".into(),
        Document::parse("link.".into()),
    );
    let counter = Counter(AtomicUsize::new(0));
    let providers: &[&dyn LinkFeature] = &[&counter];
    let request = RequestContext::new(&ws, now()).with_link_features(LinkFeatures::new(providers));
    let symbol = ws.resolve(path(), "answer").unwrap();
    let hints = wtf::presentation::hints_in(&request, path(), all());
    assert!(
        serde_json::to_string(&hints.hints)
            .unwrap()
            .contains("= 42")
    );
    assert!(wtf::intelligence::hover_in(&request, &symbol).contains("42"));
    assert!(wtf::diagnostics::collect_in(&request, path(), false).is_empty());
    let query =
        wtf::query::Query::parse("values | where name == \"answer\" | select value").unwrap();
    let result = wtf::query::execute_in(&request, &query).unwrap();
    assert_eq!(
        serde_json::to_value(result.rows).unwrap(),
        serde_json::json!([42.0])
    );
    assert_eq!(counter.0.load(Ordering::SeqCst), 1);
    let items = wtf::intelligence::completions_in(
        &request,
        Path::new("/notes/complete.wtf"),
        Position::new(0, 5),
        false,
    );
    assert!(
        items
            .iter()
            .any(|i| i.label == "answer" && i.detail.as_ref().is_some_and(|d| d.contains("42")))
    );
    let before = counter.0.load(Ordering::SeqCst);
    let fresh = RequestContext::new(&ws, now()).with_link_features(LinkFeatures::new(providers));
    fresh.engine().symbol(&symbol).unwrap();
    assert_eq!(counter.0.load(Ordering::SeqCst), before + 1);
}
#[test]
fn calendar_dates_agree_across_features_in_the_supplied_offset() {
    let ws = workspace("[day] := today()\n- [ ] Call @due(2026-09-16T10:30:00Z)\n");
    for timestamp in ["2026-09-17T00:15:00+14:00", "2026-09-15T22:15:00-12:00"] {
        let time = DateTime::parse_from_rfc3339(timestamp).unwrap();
        let request = RequestContext::new(&ws, time);
        assert_eq!(
            request.engine().named(path(), "day").unwrap(),
            Value::Date(time.date_naive())
        );
        let hints = wtf::presentation::hints_in(&request, path(), all());
        let json = serde_json::to_string(&hints.hints).unwrap();
        assert!(json.contains(&time.date_naive().to_string()), "{json}");
        assert!(json.contains("due today"), "{json}");
        let hover = wtf::intelligence::hover_in(&request, &ws.resolve(path(), "day").unwrap());
        assert!(hover.contains(&time.date_naive().to_string()));
        let query = wtf::query::Query::parse("tasks | select due").unwrap();
        let result = wtf::query::execute_in(&request, &query).unwrap();
        assert_eq!(
            serde_json::to_value(result.rows).unwrap()[0]["value"],
            time.date_naive().to_string()
        );
        assert!(wtf::diagnostics::collect_in(&request, path(), false).is_empty());
    }
}
#[test]
fn cached_errors_keep_their_source_and_do_not_poison_other_sessions() {
    let mut ws = workspace("[broken] := remote + 1\n[other] := 1 / 0\n[good] := 4\n");
    ws.documents.insert(
        "/notes/b.wtf".into(),
        Document::parse("[remote] := absent + 2\n".into()),
    );
    let request = RequestContext::new(&ws, now());
    let expected = wtf::diagnostics::collect_in(&RequestContext::new(&ws, now()), path(), false);
    wtf::presentation::hints_in(&request, path(), all());
    assert_eq!(
        wtf::diagnostics::collect_in(&request, path(), false),
        expected
    );
    let mut engine = request.engine();
    assert!(engine.named(path(), "broken").is_err());
    assert_eq!(engine.failure.unwrap().path, Path::new("/notes/b.wtf"));
    let mut clean = request.engine();
    assert_eq!(clean.named(path(), "good").unwrap(), Value::Number(4.0));
    assert!(clean.failure.is_none());
}
#[test]
fn cached_values_replay_lookup_and_clock_dependencies_without_leaking_them() {
    let mut ws = workspace("[price] := quote(ACME)\n[live] := now()\n[static] := 7\n");
    ws.lookups.insert(
        "quote:ACME".into(),
        wtf::lookups::Lookup {
            value: serde_json::json!({"price":10,"currency":"USD"}),
            fetched_at: now().to_utc(),
            source: "fixture".into(),
        },
    );
    let request = RequestContext::new(&ws, now());
    let mut warm = request.engine();
    warm.named(path(), "price").unwrap();
    warm.named(path(), "live").unwrap();
    let mut cached = request.engine();
    cached.named(path(), "price").unwrap();
    assert_eq!(cached.wanted, ["quote:ACME"]);
    assert!(!cached.time_dependent);
    cached.named(path(), "live").unwrap();
    assert!(cached.time_dependent);
    let mut unrelated = request.engine();
    unrelated.named(path(), "static").unwrap();
    assert!(unrelated.wanted.is_empty());
    assert!(!unrelated.time_dependent);
    let hover = wtf::intelligence::hover_in(&request, &ws.resolve(path(), "price").unwrap());
    assert!(hover.contains("quote ACME"));
    assert!(
        !wtf::intelligence::hover_in(&request, &ws.resolve(path(), "static").unwrap())
            .contains("quote ACME")
    );
}
