use chrono::{DateTime, FixedOffset};
use lsp_types::{InlayHintLabel, Position, Range};
use std::{collections::BTreeMap, path::Path, sync::Arc};
use wtf::{
    document::Document,
    engine::{Engine, Value},
    link_features::LinkFeature,
    modules::{Module, ModuleRegistry},
    workspace::Workspace,
};

const URL: &str = "https://issues.example/tickets/42";
const LINK: &str = r#"module := {api: 1, id: "tickets", kind: "link", hosts: ["issues.example"], properties: ["title", "points"]}
inlay := fn(ctx) => if(ctx.cached == null, "ticket " + ctx.url.path, ctx.cached.title)
hover := fn(ctx) => "Details for " + ctx.url.raw
property := fn(ctx, name) => get(ctx.cached, name)
refresh := fn(url) => {program: "/bin/echo", args: ["{\"summary\":\"Ship it\",\"points\":8}"]}
decode := fn(url, data) => {title: data.summary, points: data.points}
"#;
const INLAY: &str = r#"module := {api: 1, id: "headings", kind: "feature"}
collect := fn(ctx) => map(ctx.document.sections, fn(h) => {line: h.line, label: "section · " + h.title, tooltip: "From a functional module"})
"#;
fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-09-18T12:00:00Z").unwrap()
}
fn path() -> &'static Path {
    Path::new("/notes/main.wtf")
}
fn registry(source: &str) -> Arc<ModuleRegistry> {
    Arc::new(
        ModuleRegistry::compile([("/notes/.wtf/modules/tickets.wtf".into(), source.into())].into())
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
        modules: registry(LINK),
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
    ws.modules = registry(&LINK.replace("api: 1,", "api: 1, cache_version: 2,"));
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
    ws.modules = registry(INLAY);
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
    ws.modules = registry(&INLAY.replace("h.line", "999"));
    assert!(labels(&ws).iter().any(|s| s == "module error · headings"));
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
        assert!(ModuleRegistry::compile([("/modules/a.wtf".into(), source)].into()).is_err());
    }
    let source = LINK.replace(
        "if(ctx.cached == null, \"ticket \" + ctx.url.path, ctx.cached.title)",
        "text(ctx.file.exists)",
    );
    let module = Module::compile("/modules/a.wtf".into(), source).unwrap();
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
    let module = Module::compile("/modules/a.wtf".into(), recursive).unwrap();
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
        ModuleRegistry::compile(
            [
                ("/a.wtf".into(), LINK.into()),
                ("/b.wtf".into(), LINK.into())
            ]
            .into()
        )
        .unwrap_err()
        .contains("Duplicate module id")
    );
}

#[cfg(feature = "native")]
#[tokio::test]
async fn native_load_refresh_reload_rollback_and_removal() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    std::fs::create_dir_all(root.join(".wtf/modules")).unwrap();
    let module = root.join(".wtf/modules/tickets.wtf");
    std::fs::write(&module, LINK).unwrap();
    std::fs::write(root.join("main.wtf"), format!("{URL}:ticket\n")).unwrap();
    let mut ws = Workspace::load(vec![root.clone()]).unwrap();
    assert_eq!(
        ws.documents.len(),
        1,
        "Module definitions must stay out of note scope"
    );
    assert!(wtf::cli::refresh(&mut ws).await.is_empty());
    assert_eq!(ws.cache[URL].data.as_ref().unwrap()["title"], "Ship it");
    let snapshot = ws.clone();
    ws.reload_modules().unwrap();
    assert!(Arc::ptr_eq(&snapshot.modules, &ws.modules));
    std::fs::write(
        &module,
        LINK.replace("ctx.cached.title)", "upper(ctx.cached.title))"),
    )
    .unwrap();
    ws.reload_modules().unwrap();
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
    let working = ws.modules.clone();
    std::fs::write(&module, "module := {").unwrap();
    assert!(ws.reload_modules().is_err());
    assert!(Arc::ptr_eq(&working, &ws.modules));
    std::fs::remove_file(&module).unwrap();
    ws.reload_modules().unwrap();
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
fn browser_modules_reload_and_decode_host_data_without_executing_commands() {
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
            "setModules",
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
            "setModules",
            serde_json::json!({"sources":{"tickets.wtf":"module := {"}})
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
            "setModules",
            serde_json::json!({"sources":BTreeMap::<String,String>::new()})
        )["ok"],
        true
    );
}

#[cfg(feature = "native")]
#[test]
fn example_workspace_uses_modules_without_rust_registration() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/modules");
    let ws = Workspace::load(vec![root.clone()]).unwrap();
    assert_eq!(ws.modules.modules.len(), wtf::modules::bundled().len() + 2);
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

#[test]
fn semantic_inputs_and_utf16_anchors_are_shared_with_queries() {
    let mut ws = workspace();
    ws.documents
        .insert(path().into(), Document::parse("🦀 [round(2.6)]\n".into()));
    ws.modules = registry(
        r#"module := {api: 1, id: "calculations", kind: "feature", inputs: ["calculations"]}
collect := fn(ctx) => map(ctx.document.calculations, fn(c) => {at: c.anchor, label: "custom " + c.display, tooltip: trim(c.expression)})
"#,
    );
    let hints = wtf::presentation::hints_at(
        &ws,
        path(),
        now(),
        Range::new(Position::new(0, 0), Position::new(99, 0)),
    );
    assert_eq!(hints.len(), 1);
    assert_eq!(hints[0].position, Position::new(0, 15));
    assert!(matches!(&hints[0].label, InlayHintLabel::String(s) if s == "custom 3"));
    let q = wtf::query::Query::parse("calculations | select anchor").unwrap();
    let result = wtf::query::execute(&ws, &q, &wtf::query::QueryContext::new(now())).unwrap();
    assert_eq!(
        serde_json::to_value(result).unwrap()["rows"][0],
        serde_json::json!({"line":0,"character":15})
    );
    ws.modules =
        registry(r#"module := {api: 1, id: "calculations", kind: "feature", enabled: false}"#);
    assert!(labels(&ws).is_empty());
}

#[test]
fn module_edit_actions_share_codec_validation_and_stale_source_checks() {
    use wtf::commands::{Action, Capabilities, PreparedAction};
    let mut ws = workspace();
    ws.modules = registry(
        r#"module := {api: 1, id: "insert", kind: "feature", inputs: []}
actions := fn(ctx) => if(ctx.row == 0, [{title: "Insert greeting", action: {kind: "edit", document: ctx.document.uri, expected: ctx.document.text, edits: [{range: {start: {line: 0, character: 0}, end: {line: 0, character: 0}}, newText: "Hello "}]}}], [])
"#,
    );
    let request = wtf::RequestContext::new(&ws, now());
    let commands =
        wtf::interaction::row_commands_for(&request, path(), 0, true, Capabilities::BROWSER);
    let command = commands
        .iter()
        .find(|c| c.title == "Insert greeting")
        .unwrap();
    let action = Action::decode(&command.command, command.arguments.as_ref().unwrap()).unwrap();
    let PreparedAction::Edit { edits, .. } =
        action.prepare(&request, Capabilities::BROWSER).unwrap()
    else {
        panic!()
    };
    assert!(
        wtf::actions::apply_edits(&ws.documents[path()].text, &edits)
            .unwrap()
            .starts_with("Hello # Hello")
    );
    ws.documents
        .insert(path().into(), Document::parse("changed\n".into()));
    assert!(
        action
            .prepare(&wtf::RequestContext::new(&ws, now()), Capabilities::BROWSER)
            .unwrap_err()
            .contains("Source changed")
    );
}

#[test]
fn invalid_module_positions_are_atomic_and_reversed_edits_return_errors() {
    let mut ws = workspace();
    ws.modules = registry(
        r#"module := {api: 1, id: "bad", kind: "feature", inputs: []}
collect := fn(ctx) => [{line: 0, label: "partial"}, {at: {line: 0, character: 9}, label: "splits emoji"}]
"#,
    );
    let values = labels(&ws);
    assert!(!values.iter().any(|s| s == "partial"));
    assert!(values.iter().any(|s| s == "module error · bad"));
    let edit = lsp_types::TextEdit {
        range: Range::new(Position::new(0, 1), Position::new(0, 0)),
        new_text: String::new(),
    };
    assert!(
        wtf::actions::apply_edits("text", &[edit])
            .unwrap_err()
            .contains("Reversed")
    );
}

#[test]
fn link_callbacks_narrow_matches_properties_and_refresh_options() {
    let source=LINK.replace("inlay :=", "matches := fn(url) => ends_with(url.path, \"42\")\nproperty_names := fn(url) => [\"title\"]\ntime_dependent := fn(ctx) => false\ninlay :=").replace("program: \"/bin/echo\",", "title: \"Fetch ticket\", env: {TICKET_MODE: \"json\"}, program: \"/bin/echo\",");
    let mut ws = workspace();
    ws.modules = registry(&source);
    let links = ws.link_features();
    assert_eq!(links.property_names(URL), ["title"]);
    assert!(
        links
            .property_names("https://issues.example/tickets/43")
            .is_empty()
    );
    let refresh = links.refresh_request(URL).unwrap();
    assert_eq!(refresh.title, "Fetch ticket");
    assert_eq!(refresh.env, [("TICKET_MODE".into(), "json".into())]);
    assert!(!links.time_dependent(URL, &ws.cache, now().to_utc()));
}

#[test]
fn bundled_providers_are_replaceable_disableable_and_restored_on_unload() {
    let mut ws = workspace();
    ws.modules = Default::default();
    let url = "https://github.com/org/repo/pull/42";
    let original = ws
        .link_features()
        .presentation(url, &ws.cache, now().to_utc())
        .unwrap()
        .label;
    ws.modules = registry(
        r#"module := {api: 1, id: "github", kind: "link", hosts: ["github.com"]}
inlay := fn(ctx) => "My GitHub"
"#,
    );
    assert_eq!(
        ws.link_features()
            .presentation(url, &ws.cache, now().to_utc())
            .unwrap()
            .label,
        "My GitHub"
    );
    ws.modules = registry(r#"module := {api: 1, id: "github", kind: "link", enabled: false}"#);
    assert!(
        ws.link_features()
            .presentation(url, &ws.cache, now().to_utc())
            .is_none()
    );
    ws.modules = Default::default();
    assert_eq!(
        ws.link_features()
            .presentation(url, &ws.cache, now().to_utc())
            .unwrap()
            .label,
        original
    );
}

#[test]
fn bundled_checklist_projections_handle_large_notes_within_the_language_limits() {
    let mut ws = workspace();
    let source = format!(
        "# Large checklist\n{}",
        "- [ ] Task @estimate(1m)\n".repeat(500)
    );
    ws.documents.insert(path().into(), Document::parse(source));
    ws.modules = Default::default();
    let labels = labels(&ws);
    assert!(
        labels.iter().any(|s| s.contains("0/500 complete")),
        "{:?}",
        labels.first()
    );
    assert!(!labels.iter().any(|s| s.contains("module error")));
}

#[cfg(feature = "browser")]
#[test]
fn browser_executes_user_module_edits_and_replaces_bundled_features() {
    use serde_json::json;
    let mut browser = wtf::browser::BrowserWorkspace::new();
    let request = |b: &mut wtf::browser::BrowserWorkspace,
                   method: &str,
                   params: serde_json::Value|
     -> serde_json::Value {
        serde_json::from_str(&b.request(method, &params.to_string(), "2026-09-18T12:00:00Z"))
            .unwrap()
    };
    assert_eq!(
        request(
            &mut browser,
            "setDocument",
            json!({"uri":"file:///workspace/main.wtf","text":"Hello [round(2.6)]\n","version":1})
        )["ok"],
        true
    );
    let module = r#"module := {api: 1, id: "calculations", kind: "feature", inputs: []}
collect := fn(ctx) => [{line: 0, label: "custom"}]
actions := fn(ctx) => [{title: "Replace", action: {kind: "edit", document: ctx.document.uri, expected: ctx.document.text, edits: [{range: {start: {line: 0, character: 0}, end: {line: 0, character: 5}}, newText: "Goodbye"}]}}]
"#;
    assert_eq!(
        request(
            &mut browser,
            "setModules",
            json!({"sources":{"custom.wtf":module}})
        )["ok"],
        true
    );
    let analysis = request(
        &mut browser,
        "analyze",
        json!({"uri":"file:///workspace/main.wtf"}),
    );
    assert_eq!(analysis["ok"], true, "{analysis}");
    assert_eq!(analysis["result"]["hints"][0]["label"], "custom");
    let command = analysis["result"]["lenses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["command"]["title"] == "Replace")
        .unwrap()["command"]
        .clone();
    let executed = request(
        &mut browser,
        "execute",
        json!({"command":command,"versions":analysis["result"]["versions"]}),
    );
    assert_eq!(executed["ok"], true, "{executed}");
    let edits: Vec<lsp_types::TextEdit> =
        serde_json::from_value(executed["result"]["edit"]["documentChanges"][0]["edits"].clone())
            .unwrap();
    let text = wtf::actions::apply_edits("Hello [round(2.6)]\n", &edits).unwrap();
    assert_eq!(text, "Goodbye [round(2.6)]\n");
    assert_eq!(
        request(
            &mut browser,
            "setDocument",
            json!({"uri":"file:///workspace/main.wtf","text":text,"version":2})
        )["ok"],
        true
    );
    let stale = request(
        &mut browser,
        "execute",
        json!({"command":command,"versions":analysis["result"]["versions"]}),
    );
    assert_eq!(stale["ok"], false, "{stale}");
    assert_eq!(
        request(&mut browser, "setModules", json!({"sources":{}}))["ok"],
        true
    );
    let analysis = request(
        &mut browser,
        "analyze",
        json!({"uri":"file:///workspace/main.wtf"}),
    );
    assert_eq!(analysis["result"]["hints"][0]["label"], "3");
}

#[test]
fn bundled_consumers_relink_transitive_dependencies_and_revisions() {
    let original = ModuleRegistry::default();
    let changed = include_str!("../stdlib/format.wtf").replace("repeat(\"█\",", "repeat(\"▓\",");
    let updated =
        ModuleRegistry::compile([("/notes/.wtf/modules/format.wtf".into(), changed)].into())
            .unwrap();
    let find = |registry: &ModuleRegistry, id: &str| {
        registry.active().find(|m| m.id == id).unwrap().revision()
    };
    assert_ne!(find(&original, "format"), find(&updated, "format"));
    assert_ne!(find(&original, "timer"), find(&updated, "timer"));
    assert_ne!(find(&original, "timers"), find(&updated, "timers"));
    assert_eq!(find(&original, "github"), find(&updated, "github"));
    let mut ws = workspace();
    ws.documents.insert(
        path().into(),
        Document::parse("watch := countdown(1m)\n".into()),
    );
    ws.modules = Arc::new(updated);
    let html = wtf::rendering::html_in(&wtf::RequestContext::new(&ws, now()), path()).unwrap();
    // The bundled timers feature imports timer, which imports the replaced format.
    assert!(html.contains("░░░░░░░░"));
    ws.documents.insert(
        path().into(),
        Document::parse("watch := countdown(1m, 30s)\n".into()),
    );
    let html = wtf::rendering::html_in(&wtf::RequestContext::new(&ws, now()), path()).unwrap();
    assert!(html.contains("▓▓▓▓░░░░"), "{html}");
}

#[cfg(feature = "native")]
#[test]
fn source_library_reloads_on_disk_and_workspace_modules_take_precedence() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir(root.join("stdlib")).unwrap();
    std::fs::create_dir_all(root.join(".wtf/modules")).unwrap();
    std::fs::write(root.join("main.wtf"), "# Note\n").unwrap();
    let source = |label: &str| {
        format!(
            "module := {{api: 1, id: \"headings\", kind: \"feature\", inputs: []}}\ncollect := fn(ctx) => [{{line: 0, label: \"{label}\"}}]\n"
        )
    };
    std::fs::write(root.join("stdlib/headings.wtf"), source("library")).unwrap();
    let mut ws = Workspace::load(vec![root.into()]).unwrap();
    assert_eq!(ws.documents.len(), 1);
    let render = |ws: &Workspace| {
        wtf::rendering::html_in(&wtf::RequestContext::new(ws, now()), &root.join("main.wtf"))
            .unwrap()
    };
    assert!(render(&ws).contains(" library</span>"));
    std::fs::write(root.join("stdlib/headings.wtf"), source("saved")).unwrap();
    ws.reload_modules().unwrap();
    assert!(render(&ws).contains(" saved</span>"));
    std::fs::write(root.join(".wtf/modules/headings.wtf"), source("workspace")).unwrap();
    ws.reload_modules().unwrap();
    assert!(render(&ws).contains(" workspace</span>"));
    assert!(!render(&ws).contains(" saved</span>"));
}
