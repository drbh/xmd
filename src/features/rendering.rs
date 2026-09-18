//! Standalone HTML from the editor's semantic tokens, inlays, diagnostics and links.
use crate::{RequestContext, document::Document, presentation};
use lsp_types::{Diagnostic, DocumentLink, InlayHint, Position, Range};
use std::path::Path;
pub use wtf_renderer::{document, styles};

pub fn html_in(request: &RequestContext<'_>, path: &Path) -> Result<String, String> {
    let doc = request
        .workspace()
        .documents
        .get(path)
        .ok_or("File is not in this workspace's indexed .wtf notes")?;
    let hints = presentation::hints_in(
        request,
        path,
        Range::new(Position::new(0, 0), Position::new(u32::MAX, u32::MAX)),
    );
    html(
        &path.file_name().unwrap_or_default().to_string_lossy(),
        doc,
        &hints.hints,
        &crate::diagnostics::collect_in(request, path, false),
        &presentation::document_links_in(request, path),
    )
}

/// HTML-specific work lives in web/renderer; this adapter gathers engine data.
pub fn fragment(
    doc: &Document,
    hints: &[InlayHint],
    diagnostics: &[Diagnostic],
    links: &[DocumentLink],
) -> Result<String, String> {
    wtf_renderer::fragment(&wtf_renderer::Snapshot {
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
pub fn html(
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
