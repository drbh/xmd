//! Standalone HTML from the editor's semantic tokens, inlays, diagnostics and links.
use crate::view::presentation;
use eval::RequestContext;
use lsp_types::{Diagnostic, DocumentLink, InlayHint, Position, Range};
use model::Document;
use renderer::document;
use std::path::Path;

pub(crate) fn html_for(request: &RequestContext<'_>, path: &Path) -> Result<String, String> {
    let doc = request.workspace().documents.get(path).ok_or_else(|| {
        format!(
            "File is not in this workspace's indexed .{} notes",
            common::EXTENSION
        )
    })?;
    let hints = presentation::hints(
        request,
        path,
        Range::new(Position::new(0, 0), Position::new(u32::MAX, u32::MAX)),
    );
    html(
        &path.file_name().unwrap_or_default().to_string_lossy(),
        doc,
        &hints.hints,
        &crate::providers::diagnostics(request, path, false),
        &presentation::document_links(request, path),
    )
}

/// HTML-specific work lives in lang/renderer; this adapter gathers engine data.
pub fn fragment(
    doc: &Document,
    hints: &[InlayHint],
    diagnostics: &[Diagnostic],
    links: &[DocumentLink],
) -> Result<String, String> {
    renderer::fragment(&renderer::Snapshot {
        source: &doc.text,
        tokens: &presentation::semantic_tokens(doc),
        token_types: presentation::TOKEN_TYPES,
        token_modifiers: presentation::TOKEN_MODIFIERS,
        hints,
        diagnostics,
        links,
        line_classes: &line_classes(doc),
    })
}
fn html(
    title: &str,
    doc: &Document,
    hints: &[InlayHint],
    diagnostics: &[Diagnostic],
    links: &[DocumentLink],
) -> Result<String, String> {
    Ok(document(title, &fragment(doc, hints, diagnostics, links)?))
}

/// Block styling follows the parser, including its fenced-code/comment context.
pub fn line_classes(doc: &Document) -> Vec<String> {
    let mut lines = vec![String::new(); doc.text.split('\n').count()];
    for section in &doc.sections {
        lines[section.line] = format!("h{}", section.level);
    }
    for task in &doc.tasks {
        lines[task.line] = "task".into();
    }
    lines
}
