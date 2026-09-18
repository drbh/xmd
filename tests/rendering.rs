use chrono::{DateTime, FixedOffset};
use lsp_types::{InlayHint, InlayHintLabel, InlayHintLabelPart, Position, Range, TextEdit};
use std::path::Path;
use wtf::{RequestContext, document::Document, presentation, workspace::Workspace};

fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-09-18T12:00:00Z").unwrap()
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

#[test]
fn resolved_text_preserves_source_and_places_multiline_and_inline_results() {
    let source =
        "[$10]:budget\n[total] := (\n  budget * 2\n)\n🦀 Total [total], extra [total + $1].";
    let expected = "[$10]:budget\n[total] := (\n  budget * 2\n) = $20\n🦀 Total [total] $20, extra [total + $1] $21.";
    for ending in ["\n", "\r\n"] {
        for trailing in ["", ending] {
            let source = source.replace('\n', ending) + trailing;
            let ws = note(&source);
            assert_eq!(
                RequestContext::new(&ws, now()).render_text(path()).unwrap(),
                expected.replace('\n', ending) + trailing
            );
            assert_eq!(ws.documents[path()].text, source);
        }
    }
}

#[test]
fn label_parts_padding_and_ties_use_display_order_without_applying_actions() {
    let hint = |position, label| InlayHint {
        position,
        label: InlayHintLabel::String(label),
        kind: None,
        text_edits: None,
        tooltip: None,
        padding_left: None,
        padding_right: None,
        data: None,
    };
    let mut parts = hint(Position::new(0, 2), String::new());
    parts.label = InlayHintLabel::LabelParts(
        ["first", "\npart"]
            .into_iter()
            .map(|value| InlayHintLabelPart {
                value: value.into(),
                tooltip: None,
                location: None,
                command: None,
            })
            .collect(),
    );
    parts.padding_left = Some(true);
    parts.padding_right = Some(true);
    parts.text_edits = Some(vec![TextEdit {
        range: Range::new(Position::new(0, 0), Position::new(0, 2)),
        new_text: "DO NOT APPLY".into(),
    }]);
    let hints = [
        hint(Position::new(2, 0), "end".into()),
        parts,
        hint(Position::new(0, 2), "second".into()),
        hint(Position::new(1, 0), "empty".into()),
    ];
    assert_eq!(
        presentation::render_text("🦀!\r\n\r\n", &hints).unwrap(),
        "🦀 first part second!\r\nempty\r\nend"
    );
    assert!(presentation::render_text("🦀", &[hint(Position::new(0, 1), "bad".into())]).is_err());
    assert!(presentation::render_text("", &[hint(Position::new(1, 0), "bad".into())]).is_err());
    assert_eq!(presentation::render_text("", &[]).unwrap(), "");
    let document = Document::parse("🦀!\r\n\r\n".into());
    let html = wtf::rendering::html("Escaped <&\" title", &document, &hints, &[], &[]).unwrap();
    assert!(html.contains("<title>Escaped &lt;&amp;&quot; title</title>"));
    assert!(html.contains("🦀<span class=\"inlay\" contenteditable=\"false\" title=\"\"> first part </span><span class=\"inlay\" contenteditable=\"false\" title=\"\">second</span>!\r\n"));
    assert!(!html.contains("DO NOT APPLY"));
    assert!(
        wtf::rendering::html(
            "bad",
            &document,
            &[hint(Position::new(0, 1), "bad".into())],
            &[],
            &[]
        )
        .is_err()
    );
}

#[test]
fn resolved_text_uses_workspace_modules_overrides_and_the_request_clock() {
    let mut ws = note("clock := now()\n");
    ws.modules = std::sync::Arc::new(wtf::modules::ModuleRegistry::compile([
        ("/notes/.wtf/modules/definitions.wtf".into(), "module := {api: 1, id: \"definitions\", kind: \"feature\", enabled: false}".into()),
        ("/notes/.wtf/modules/custom.wtf".into(), "module := {api: 1, id: \"custom\", kind: \"feature\", inputs: []}\ncollect := fn(ctx) => [{line: 0, label: source(now())}, {line: 0, label: \"second\"}]".into()),
    ].into()).unwrap());
    assert_eq!(
        RequestContext::new(&ws, now()).render_text(path()).unwrap(),
        "clock := now() 2026-09-18T12:00:00+00:00 second\n"
    );
    assert!(
        RequestContext::new(&ws, now())
            .render_text(Path::new("/notes/missing.wtf"))
            .is_err()
    );
}

#[test]
fn resolved_text_matches_existing_feature_snapshots() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/lifted-features.json")).unwrap();
    let clock = DateTime::parse_from_rfc3339("2026-09-16T14:00:00-04:00").unwrap();
    for fixture in fixtures.as_array().unwrap() {
        let source = fixture["source"].as_str().unwrap();
        let ws = note(source);
        let hints: Vec<InlayHint> = serde_json::from_value(fixture["hints"].clone()).unwrap();
        assert_eq!(
            RequestContext::new(&ws, clock).render_text(path()).unwrap(),
            presentation::render_text(source, &hints).unwrap()
        );
        let html = RequestContext::new(&ws, clock).render_html(path()).unwrap();
        assert_eq!(html.matches("class=\"inlay\"").count(), hints.len());
    }
}

#[test]
fn html_escapes_content_tooltips_and_links_and_marks_diagnostics() {
    use lsp_types::{Diagnostic, DocumentLink, InlayHintTooltip, Url};
    let doc = Document::parse("abc 🦀\n<script>bad()</script>\n".into());
    let range = Range::new(Position::new(0, 0), Position::new(0, 3));
    let mut hint = InlayHint {
        position: Position::new(0, 3),
        label: InlayHintLabel::String("<img src=x onerror=bad()>".into()),
        kind: None,
        text_edits: None,
        tooltip: Some(InlayHintTooltip::String("\" onmouseover=\"bad()".into())),
        padding_left: Some(true),
        padding_right: None,
        data: None,
    };
    let diagnostic = Diagnostic::new_simple(range, "<error & detail>".into());
    let point = Diagnostic::new_simple(
        Range::new(Position::new(2, 0), Position::new(2, 0)),
        "Expected a value".into(),
    );
    let link = |target| DocumentLink {
        range,
        target: Some(Url::parse(target).unwrap()),
        tooltip: None,
        data: None,
    };
    let html = wtf::rendering::html(
        "test",
        &doc,
        &[hint.clone()],
        &[diagnostic, point],
        &[
            link("javascript:bad()"),
            link("https://example.com/?a=1&b=2"),
        ],
    )
    .unwrap();
    assert!(html.contains("diagnostic error"));
    assert!(html.contains("class=\"diagnostic error point\" title=\"Expected a value\"></span>"));
    assert!(html.contains("&lt;error &amp; detail&gt;"));
    assert!(html.contains("href=\"https://example.com/?a=1&amp;b=2\""));
    assert!(!html.contains("javascript:"));
    assert!(!html.contains("<script>"));
    assert!(!html.contains("<img"));
    assert!(html.contains("&lt;img src=x onerror=bad()&gt;"));
    assert!(html.contains("title=\"&quot; onmouseover=&quot;bad()\""));
    hint.position = Position::new(99, 0);
    assert!(wtf::rendering::html("test", &doc, &[hint], &[], &[]).is_err());
    assert!(
        wtf::rendering::html("empty", &Document::parse(String::new()), &[], &[], &[])
            .unwrap()
            .contains("<code><span class=\"line \" data-line=\"0\"></span></code>")
    );
}

#[test]
fn standard_library_html_matches_the_pre_migration_output() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/stdlib-html.json")).unwrap();
    let clock = DateTime::parse_from_rfc3339("2026-09-16T14:00:00-04:00").unwrap();
    for fixture in fixtures.as_array().unwrap() {
        let ws = note(fixture["source"].as_str().unwrap());
        let request = RequestContext::new(&ws, clock);
        let hints = request.hints(
            path(),
            Range::new(Position::new(0, 0), Position::new(u32::MAX, u32::MAX)),
        );
        let fragment = wtf::rendering::fragment(
            &ws.documents[path()],
            &hints.hints,
            &request.diagnostics(path(), false),
            &request.document_links(path()),
        )
        .unwrap();
        assert_eq!(
            fragment,
            fixture["html"].as_str().unwrap(),
            "{}",
            fixture["source"]
        );
    }
}
