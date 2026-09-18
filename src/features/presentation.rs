//! Host-independent editor presentation, shared by native LSP and WebAssembly.
use crate::{
    document::{Problem, Span},
    engine::{Engine, Value},
    workspace::{Symbol, SymbolKind, Workspace},
};
use chrono::{DateTime, FixedOffset, NaiveDate};
use lsp_types::*;
use std::path::Path;

pub use crate::highlighting::{TOKEN_MODIFIERS, TOKEN_TYPES, semantic_tokens};

pub fn document_links(
    workspace: &Workspace,
    path: &Path,
    now: DateTime<FixedOffset>,
) -> Vec<DocumentLink> {
    document_links_in(&crate::RequestContext::new(workspace, now), path)
}
pub fn document_links_in(request: &crate::RequestContext<'_>, path: &Path) -> Vec<DocumentLink> {
    let workspace = request.workspace();

    let Some(doc) = workspace.documents.get(path) else {
        return vec![];
    };
    let mut links = Vec::new();
    let mut add = |span: Span, resource: crate::resources::Resource| {
        if let Ok(url) = resource.url(path) {
            links.push(DocumentLink {
                range: span.range(&doc.text),
                tooltip: Some(format!("Open {url}")),
                target: Some(url),
                data: None,
            });
        }
    };
    for link in &doc.links {
        add(
            link.span,
            crate::resources::Resource {
                target: link.target.clone(),
                origin: None,
            },
        );
    }
    let mut engine = request.engine();
    for (i, def) in doc.definitions.iter().enumerate() {
        if let Ok(Value::Resource(resource)) = engine.symbol(&Symbol {
            path: path.into(),
            kind: SymbolKind::Definition(i),
        }) {
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
pub fn problems(workspace: &Workspace, path: &Path, today: NaiveDate) -> Vec<Problem> {
    crate::diagnostics::problems(workspace, path, today)
}
pub fn hints(workspace: &Workspace, path: &Path, today: NaiveDate, range: Range) -> Vec<InlayHint> {
    let mut engine = Engine::new(workspace, today);
    crate::inlays::collect(&mut engine, path, range, crate::inlay_providers::BUILTINS).hints
}
pub fn hints_at(
    workspace: &Workspace,
    path: &Path,
    now: DateTime<FixedOffset>,
    range: Range,
) -> Vec<InlayHint> {
    hints_in(&crate::RequestContext::new(workspace, now), path, range).hints
}
pub fn hints_in(
    request: &crate::RequestContext<'_>,
    path: &Path,
    range: Range,
) -> crate::inlays::InlayOutput {
    crate::inlays::collect(
        &mut request.engine(),
        path,
        range,
        crate::inlay_providers::BUILTINS,
    )
}

/// Materialize the same source and inline labels shown by the editor, at one clock snapshot.
pub fn render_text_in(request: &crate::RequestContext<'_>, path: &Path) -> Result<String, String> {
    let doc = request
        .workspace()
        .documents
        .get(path)
        .ok_or("File is not in this workspace's indexed .wtf notes")?;
    let hints = hints_in(
        request,
        path,
        Range::new(Position::new(0, 0), Position::new(u32::MAX, u32::MAX)),
    );
    render_text(&doc.text, &hints.hints)
}

/// Insert display labels, honoring UTF-16 positions, padding and provider order.
/// Inlay text edits are interactive actions and are never applied by a renderer.
pub fn render_text(source: &str, hints: &[InlayHint]) -> Result<String, String> {
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
    crate::actions::apply_edits(source, &edits)
}

pub(crate) fn inlay_label(hint: &InlayHint) -> String {
    let label = match &hint.label {
        InlayHintLabel::String(text) => text.clone(),
        InlayHintLabel::LabelParts(parts) => parts.iter().map(|p| p.value.as_str()).collect(),
    };
    // LSP labels occupy one display line even if their value contains newlines.
    label.replace(['\r', '\n', '\t'], " ")
}

pub fn live_hints(workspace: &Workspace, path: &Path, now: DateTime<FixedOffset>) -> bool {
    live_hints_in(&crate::RequestContext::new(workspace, now), path)
}
pub fn live_hints_in(request: &crate::RequestContext<'_>, path: &Path) -> bool {
    hints_in(
        request,
        path,
        Range::new(Position::new(0, 0), Position::new(u32::MAX, 0)),
    )
    .time_dependent
}
