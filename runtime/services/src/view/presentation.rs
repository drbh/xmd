//! Host-independent editor presentation, shared by native LSP and WebAssembly.
use lang::common::Span;
use lang::eval::engine::Value;
use lang::eval::{Symbol, SymbolKind};
use lsp_types::*;
use std::path::Path;

pub use crate::view::highlighting::{TOKEN_MODIFIERS, TOKEN_TYPES, semantic_tokens};

pub(crate) fn document_links(
    request: &lang::eval::RequestContext<'_>,
    path: &Path,
) -> Vec<DocumentLink> {
    let workspace = request.workspace();

    let Some(doc) = workspace.documents().get(path) else {
        return vec![];
    };
    let mut links = Vec::new();
    let mut add = |span: Span, resource: lang::eval::resources::Resource| {
        if let Ok(url) = resource.url(path) {
            links.push(DocumentLink {
                range: span.range(&doc.text),
                tooltip: Some(format!("Open {url}")),
                target: Some(lang::common::uri_from_url(&url)),
                data: None,
            });
        }
    };
    for link in &doc.links {
        add(
            link.span,
            lang::eval::resources::Resource {
                target: link.target.clone(),
                origin: None,
            },
        );
    }
    let mut engine = request.engine();
    for (i, def) in doc.definitions.iter().enumerate() {
        if let Ok(Value::Resource(resource)) =
            engine.symbol(&Symbol::new(path, SymbolKind::Definition(i)))
        {
            add(def.value_span, resource);
        }
    }
    for reference in &doc.references {
        if let Ok(Value::Resource(resource)) = engine.eval(path, &reference.expression()) {
            add(
                Span::new(reference.span.line, reference.span.start, reference.end()),
                resource,
            );
        }
    }
    links.sort_by_key(|link| (link.range.start, link.range.end));
    links.dedup_by(|a, b| a.range == b.range && a.target == b.target);
    links
}
pub(crate) fn hints(
    request: &lang::eval::RequestContext<'_>,
    path: &Path,
    range: Range,
) -> crate::view::inlays::InlayOutput {
    crate::view::inlays::collect(request, path, range)
}

/// Materialize the same source and inline labels shown by the editor, at one clock snapshot.
pub(crate) fn rendered_text(
    request: &lang::eval::RequestContext<'_>,
    path: &Path,
) -> Result<String, String> {
    let doc = request.workspace().documents().get(path).ok_or_else(|| {
        format!(
            "File is not in this workspace's indexed .{} notes",
            lang::common::EXTENSION
        )
    })?;
    let hints = hints(
        request,
        path,
        Range::new(Position::new(0, 0), Position::new(u32::MAX, u32::MAX)),
    );
    render_text(&doc.text, &hints.hints)
}

/// Insert display labels, honoring UTF-16 positions, padding and provider order.
/// Inlay text edits are interactive actions and are never applied by a renderer.
pub(crate) fn render_text(source: &str, hints: &[InlayHint]) -> Result<String, String> {
    let edits: Vec<_> = hints
        .iter()
        .map(|hint| {
            let mut label = String::new();
            if hint.padding_left == Some(true) {
                label.push(' ');
            }
            label.push_str(&inlay_label(hint));
            if hint.padding_right == Some(true) {
                label.push(' ');
            }
            TextEdit {
                range: Range::new(hint.position, hint.position),
                new_text: label,
            }
        })
        .collect();
    crate::controls::code_actions::apply_edits(source, &edits)
}

pub(crate) fn inlay_label(hint: &InlayHint) -> String {
    let label = match &hint.label {
        InlayHintLabel::String(text) => text.clone(),
        InlayHintLabel::LabelParts(parts) => parts.iter().map(|p| p.value.as_str()).collect(),
    };
    // LSP labels occupy one display line even if their value contains newlines.
    label.replace(['\r', '\n', '\t'], " ")
}

pub(crate) fn live_hints(request: &lang::eval::RequestContext<'_>, path: &Path) -> bool {
    hints(
        request,
        path,
        Range::new(Position::new(0, 0), Position::new(u32::MAX, 0)),
    )
    .time_dependent
}
