//! Links: every span in a note that opens a resource.
use lang::common::Span;
use lang::eval::engine::Value;
use lang::eval::resources::Resource;
use lang::eval::{Symbol, SymbolKind};
use lsp_types::DocumentLink;
use std::path::Path;

pub(crate) fn document_links(
    request: &lang::eval::RequestContext<'_>,
    path: &Path,
) -> Vec<DocumentLink> {
    let workspace = request.workspace();

    let Some(doc) = workspace.documents().get(path) else {
        return vec![];
    };
    let mut links = Vec::new();
    let mut add = |span: Span, resource: Resource| {
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
            Resource {
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
