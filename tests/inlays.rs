use chrono::{DateTime, FixedOffset};
use lsp_types::{InlayHintLabel, InlayHintTooltip, Position, Range};
use std::path::Path;
use wtf::{
    document::Document,
    engine::Engine,
    inlays::{self, InlayContext, InlayFeature, InlaySink},
    workspace::Workspace,
};
fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-09-16T12:00:00-04:00").unwrap()
}
fn path() -> &'static Path {
    Path::new("/notes/n.wtf")
}
fn workspace() -> Workspace {
    Workspace {
        roots: vec!["/notes".into()],
        documents: [(
            path().into(),
            Document::parse("# 🦀 Trip\n- [ ] Pack @due(tomorrow)\nclock := now()\n".into()),
        )]
        .into(),
        cache: Default::default(),
        lookups: Default::default(),
        plugins: Default::default(),
    }
}
fn all() -> Range {
    Range::new(Position::new(0, 0), Position::new(u32::MAX, 0))
}
fn label(hint: &lsp_types::InlayHint) -> &str {
    let InlayHintLabel::String(label) = &hint.label else {
        panic!("expected text")
    };
    label
}

/// A new, non-link feature needs only the core trait; no host dispatch changes.
struct HeadingClock;
impl InlayFeature for HeadingClock {
    fn collect(&self, ctx: &mut InlayContext<'_, '_>, out: &mut InlaySink) {
        ctx.mark_time_dependent();
        for heading in &ctx.document.sections {
            out.push(
                ctx.document.line_end(heading.line),
                ctx.engine.now.format("%H:%M").to_string(),
                "Clock from this request".into(),
            );
        }
    }
}
struct AtLine(usize, &'static str);
impl InlayFeature for AtLine {
    fn collect(&self, ctx: &mut InlayContext<'_, '_>, out: &mut InlaySink) {
        out.push(
            ctx.document.line_end(self.0),
            self.1.into(),
            "**Tooltip**".into(),
        );
    }
}
#[test]
fn a_nonlink_feature_composes_with_builtins_and_uses_the_request_clock() {
    let ws = workspace();
    let mut engine = Engine::at(&ws, now());
    let mut features = wtf::inlay_providers::BUILTINS.to_vec();
    features.push(&HeadingClock);
    let result = inlays::collect(&mut engine, path(), all(), &features);
    assert!(result.time_dependent);
    let clock = result
        .hints
        .iter()
        .find(|hint| label(hint) == "12:00")
        .unwrap();
    assert_eq!(clock.position, Position::new(0, 9)); // UTF-16, including the crab's surrogate pair.
    assert!(
        result
            .hints
            .iter()
            .any(|hint| label(hint).contains("due tomorrow"))
    );
    assert!(
        result
            .hints
            .iter()
            .any(|hint| label(hint).contains("complete"))
    );
    assert!(result.hints.iter().all(|hint| hint.text_edits.is_none()));
}
#[test]
fn every_producer_uses_central_range_filtering_ordering_and_tooltips() {
    let ws = workspace();
    let mut engine = Engine::at(&ws, now());
    let range = Range::new(Position::new(0, 9), ws.documents[path()].line_end(1));
    let result = inlays::collect(
        &mut engine,
        path(),
        range,
        &[
            &AtLine(1, "late"),
            &AtLine(0, "first"),
            &AtLine(0, "second"),
            &AtLine(2, "outside"),
        ],
    );
    assert_eq!(
        result.hints.iter().map(label).collect::<Vec<_>>(),
        ["first", "second", "late"]
    );
    assert!(!result.time_dependent);
    assert!(
        matches!(&result.hints[0].tooltip,Some(InlayHintTooltip::MarkupContent(m)) if m.value=="**Tooltip**")
    );
    assert_eq!(result.hints[0].padding_left, Some(true));
    assert!(
        inlays::collect(
            &mut engine,
            Path::new("/notes/missing.wtf"),
            all(),
            &[&HeadingClock]
        )
        .hints
        .is_empty()
    );
}
