use chrono::{DateTime, FixedOffset};
use lsp_types::{InlayHintLabel, Position, Range};
use std::{collections::BTreeMap, path::Path, sync::Arc};
use wtf::{
    document::Document,
    engine::{Engine, Value},
    link_features::LinkFeature,
    plugins::{Module, Plugins},
    workspace::Workspace,
};

const URL: &str = "https://issues.example/tickets/42";
const LINK: &str = r#"plugin := {api: 1, id: "tickets", kind: "link", hosts: ["issues.example"], properties: ["title", "points"]}
inlay := fn(ctx) => if(ctx.cached == null, "ticket " + ctx.url.path, ctx.cached.title)
hover := fn(ctx) => "Details for " + ctx.url.raw
property := fn(ctx, name) => get(ctx.cached, name)
refresh := fn(url) => {program: "/bin/echo", args: ["{\"summary\":\"Ship it\",\"points\":8}"]}
decode := fn(url, data) => {title: data.summary, points: data.points}
"#;
const INLAY: &str = r#"plugin := {api: 1, id: "headings", kind: "inlay"}
collect := fn(ctx) => map(ctx.document.sections, fn(h) => {line: h.line, label: "section · " + h.title, tooltip: "From a functional plugin"})
"#;
fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-09-18T12:00:00Z").unwrap()
}
fn path() -> &'static Path {
    Path::new("/notes/main.wtf")
}
fn registry(source: &str) -> Arc<Plugins> {
    Arc::new(
        Plugins::compile([("/notes/.wtf/plugins/tickets.wtf".into(), source.into())].into())
            .unwrap(),
    )
}
fn workspace() -> Workspace {
    Workspace {
        roots: vec!["/notes".into()],
        documents: [(
            path().into(),
            Document::parse(format!(
                "# Hello 🦀\n{URL}:ticket\nSee [ticket] and [issue]({URL}).\n"
            )),
        )]
        .into(),
        cache: Default::default(),
        lookups: Default::default(),
        plugins: registry(LINK),
    }
}
fn labels(ws: &Workspace) -> Vec<String> {
    wtf::presentation::hints_at(
        ws,
        path(),
        now(),
        Range::new(Position::new(0, 0), Position::new(99, 0)),
    )
    .into_iter()
    .map(|h| match h.label {
        InlayHintLabel::String(s) => s,
        _ => panic!(),
    })
    .collect()
}

#[test]
fn functional_links_share_labels_properties_hover_and_cache_versions() {
    let mut ws = workspace();
    assert!(
        labels(&ws)
            .iter()
            .filter(|s| s.contains("ticket /tickets/42"))
            .count()
            >= 3
    );
    assert_eq!(ws.link_features().property_names(URL), ["title", "points"]);
    let metadata = ws
        .link_features()
        .decode_refresh(
            URL,
            &serde_json::json!({"summary":"Ready","points":5}),
            now().to_utc(),
        )
        .unwrap();
    ws.cache.insert(URL.into(), metadata);
    assert_eq!(
        Engine::at(&ws, now())
            .eval(path(), "ticket.points")
            .unwrap(),
        Value::Number(5.0)
    );
    assert!(labels(&ws).iter().any(|s| s == "Ready"));
    let original = ws.clone();
    ws.plugins = registry(&LINK.replace("api: 1,", "api: 1, cache_version: 2,"));
    assert!(labels(&ws).iter().any(|s| s == "ticket /tickets/42"));
    assert!(labels(&original).iter().any(|s| s == "Ready"));
    assert!(
        ws.link_features()
            .presentation(
                "https://issues.example.evil/tickets/42",
                &ws.cache,
                now().to_utc()
            )
            .is_none()
    );
}

#[test]
fn generic_inlays_use_the_shared_sink_and_do_not_edit_notes() {
    let mut ws = workspace();
    ws.plugins = registry(INLAY);
    let source = ws.documents[path()].text.clone();
    let hints = wtf::presentation::hints_at(
        &ws,
        path(),
        now(),
        Range::new(Position::new(0, 0), Position::new(0, 99)),
    );
    let hint = hints
        .iter()
        .find(|h| matches!(&h.label, InlayHintLabel::String(s) if s == "section · Hello 🦀"))
        .unwrap();
    assert_eq!(hint.position, Position::new(0, 10));
    assert_eq!(ws.documents[path()].text, source);
    assert!(
        wtf::presentation::hints_at(
            &ws,
            path(),
            now(),
            Range::new(Position::new(1, 0), Position::new(1, 0))
        )
        .is_empty()
    );
    ws.plugins = registry(&INLAY.replace("h.line", "999"));
    assert!(labels(&ws).iter().any(|s| s == "plugin error · headings"));
}

#[test]
fn invalid_modules_and_impure_access_fail_with_errors() {
    for source in [
        LINK.replace("api: 1", "api: 2"),
        LINK.replace("fn(ctx) => if", "fn() => if"),
        LINK.replace("kind: \"link\"", "kind: \"other\""),
        LINK.replace("fn(ctx) => if", "fn(ctx, ctx) => if"),
        LINK.replace("decode :=", "not_decode :="),
    ] {
        assert!(Plugins::compile([("/plugins/a.wtf".into(), source)].into()).is_err());
    }
    let source = LINK.replace(
        "if(ctx.cached == null, \"ticket \" + ctx.url.path, ctx.cached.title)",
        "text(ctx.file.exists)",
    );
    let module = Module::compile("/plugins/a.wtf".into(), source).unwrap();
    let input = Value::Record(
        [(
            "file".into(),
            Value::Resource(wtf::resources::Resource {
                target: "/etc/passwd".into(),
                origin: None,
            }),
        )]
        .into(),
    );
    assert!(
        module
            .call("inlay", vec![input], now())
            .unwrap_err()
            .contains("filesystem")
    );
    let recursive = LINK.replace(
        "if(ctx.cached == null, \"ticket \" + ctx.url.path, ctx.cached.title)",
        "inlay(ctx)",
    );
    let module = Module::compile("/plugins/a.wtf".into(), recursive).unwrap();
    let url = URL.parse().unwrap();
    assert!(
        module
            .inlay(&wtf::link_features::LinkContext {
                url: &url,
                cached: None,
                now: now().to_utc()
            })
            .contains("depth")
    );
    assert!(
        Plugins::compile(
            [
                ("/a.wtf".into(), LINK.into()),
                ("/b.wtf".into(), LINK.into())
            ]
            .into()
        )
        .unwrap_err()
        .contains("Duplicate plugin id")
    );
}

#[cfg(feature = "native")]
#[tokio::test]
async fn native_load_refresh_reload_rollback_and_removal() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    std::fs::create_dir_all(root.join(".wtf/plugins")).unwrap();
    let plugin = root.join(".wtf/plugins/tickets.wtf");
    std::fs::write(&plugin, LINK).unwrap();
    std::fs::write(root.join("main.wtf"), format!("{URL}:ticket\n")).unwrap();
    let mut ws = Workspace::load(vec![root.clone()]).unwrap();
    assert_eq!(
        ws.documents.len(),
        1,
        "Plugin definitions must stay out of note scope"
    );
    assert!(wtf::cli::refresh(&mut ws).await.is_empty());
    assert_eq!(ws.cache[URL].data.as_ref().unwrap()["title"], "Ship it");
    let snapshot = ws.clone();
    ws.reload_plugins().unwrap();
    assert!(Arc::ptr_eq(&snapshot.plugins, &ws.plugins));
    std::fs::write(
        &plugin,
        LINK.replace("ctx.cached.title)", "upper(ctx.cached.title))"),
    )
    .unwrap();
    ws.reload_plugins().unwrap();
    assert_eq!(
        ws.link_features()
            .presentation(URL, &ws.cache, now().to_utc())
            .unwrap()
            .label,
        "SHIP IT"
    );
    assert_eq!(
        snapshot
            .link_features()
            .presentation(URL, &snapshot.cache, now().to_utc())
            .unwrap()
            .label,
        "Ship it"
    );
    let working = ws.plugins.clone();
    std::fs::write(&plugin, "plugin := {").unwrap();
    assert!(ws.reload_plugins().is_err());
    assert!(Arc::ptr_eq(&working, &ws.plugins));
    std::fs::remove_file(&plugin).unwrap();
    ws.reload_plugins().unwrap();
    assert!(
        ws.link_features()
            .presentation(URL, &ws.cache, now().to_utc())
            .is_none()
    );
    let cached: wtf::resources::Cache =
        serde_json::from_slice(&std::fs::read(root.join(".wtf/cache.json")).unwrap()).unwrap();
    assert_eq!(cached[URL].provider.as_deref(), Some("tickets:1"));
}

#[cfg(feature = "browser")]
#[test]
fn browser_plugins_reload_and_decode_host_data_without_executing_commands() {
    fn request(
        browser: &mut wtf::browser::BrowserWorkspace,
        method: &str,
        params: serde_json::Value,
    ) -> serde_json::Value {
        serde_json::from_str(&browser.request(method, &params.to_string(), "2026-09-18T12:00:00Z"))
            .unwrap()
    }
    let mut browser = wtf::browser::BrowserWorkspace::new();
    assert_eq!(
        request(
            &mut browser,
            "setDocument",
            serde_json::json!({"uri":"file:///workspace/main.wtf","version":1,"text":format!("{URL}:ticket\n")})
        )["ok"],
        true
    );
    assert_eq!(
        request(
            &mut browser,
            "setPlugins",
            serde_json::json!({"sources":{"tickets.wtf":LINK}})
        )["ok"],
        true
    );
    assert_eq!(
        request(
            &mut browser,
            "setResourceData",
            serde_json::json!({"url":URL,"data":{"summary":"Browser","points":3}})
        )["ok"],
        true
    );
    let query = serde_json::json!({"query":"values | where name == \"ticket\" | select eval(\"ticket.points\")"});
    assert_eq!(
        request(&mut browser, "query", query.clone())["result"]["rows"],
        serde_json::json!([3.0])
    );
    assert_eq!(
        request(
            &mut browser,
            "setPlugins",
            serde_json::json!({"sources":{"tickets.wtf":"plugin := {"}})
        )["ok"],
        false
    );
    assert_eq!(
        request(&mut browser, "query", query)["result"]["rows"],
        serde_json::json!([3.0])
    );
    assert_eq!(
        request(
            &mut browser,
            "setPlugins",
            serde_json::json!({"sources":BTreeMap::<String,String>::new()})
        )["ok"],
        true
    );
}

#[cfg(feature = "native")]
#[test]
fn example_workspace_uses_plugins_without_rust_registration() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/plugins");
    let ws = Workspace::load(vec![root.clone()]).unwrap();
    assert_eq!(ws.plugins.modules.len(), 2);
    let hints = wtf::presentation::hints_at(
        &ws,
        &root.join("demo.wtf"),
        now(),
        Range::new(Position::new(0, 0), Position::new(99, 0)),
    );
    assert!(
        hints
            .iter()
            .any(|h| matches!(&h.label,InlayHintLabel::String(s) if s=="docs · /getting-started"))
    );
    assert!(
        hints
            .iter()
            .any(|h| matches!(&h.label,InlayHintLabel::String(s) if s=="18 characters"))
    );
}
