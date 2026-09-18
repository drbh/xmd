//! Data-only module hooks; shared by native and browser hosts.
use crate::{
    RequestContext,
    engine::Value,
    modules::{Hook, Module, ModuleKind, json},
};
use lsp_types::{
    Diagnostic, DiagnosticSeverity, Hover, HoverContents, MarkupContent, MarkupKind,
    NumberOrString, Position, Range, TextEdit,
};
use std::path::Path;
fn call(
    request: &RequestContext<'_>,
    path: &Path,
    module: &Module,
    hook: Hook,
) -> Result<Vec<serde_json::Value>, String> {
    let input = super::module_inlays::input(module, &mut request.engine(), path)?;
    let Value::List(items) = module.call(hook, vec![input], request.now())? else {
        return Err(format!("{hook} must return a list"));
    };
    items.iter().map(json).collect()
}
fn validate(request: &RequestContext<'_>, path: &Path, range: Range) -> Result<(), String> {
    crate::actions::apply_edits(
        &request.workspace().documents[path].text,
        &[TextEdit::new(range, String::new())],
    )
    .map(|_| ())
}
pub(crate) fn diagnostics(request: &RequestContext<'_>, path: &Path) -> Vec<Diagnostic> {
    if !request.workspace().documents.contains_key(path) {
        return vec![];
    }
    let mut result = vec![];
    for module in request
        .workspace()
        .modules
        .active()
        .filter(|m| m.kind == ModuleKind::Feature && m.has(Hook::Diagnostics))
    {
        let batch = (|| {
            let mut batch = vec![];
            for item in call(request, path, module, Hook::Diagnostics)? {
                let mut diagnostic: Diagnostic =
                    serde_json::from_value(item).map_err(|e| e.to_string())?;
                validate(request, path, diagnostic.range)?;
                diagnostic.source.get_or_insert("wtf".into());
                batch.push(diagnostic);
            }
            Ok::<_, String>(batch)
        })();
        match batch {
            Ok(batch) => result.extend(batch),
            Err(error) => result.push(Diagnostic {
                range: Range::default(),
                severity: Some(DiagnosticSeverity::ERROR),
                source: Some("wtf".into()),
                code: Some(NumberOrString::String("module".into())),
                message: format!("{}: {error}", module.id),
                ..Default::default()
            }),
        }
    }
    result
}
pub(crate) fn hover(
    request: &RequestContext<'_>,
    path: &Path,
    position: Position,
) -> Option<Hover> {
    request.workspace().documents.get(path)?;
    for module in request
        .workspace()
        .modules
        .active()
        .filter(|m| m.kind == ModuleKind::Feature && m.has(Hook::Hovers))
    {
        let batch = (|| {
            let mut batch = vec![];
            for item in call(request, path, module, Hook::Hovers)? {
                let range: Range =
                    serde_json::from_value(item["range"].clone()).map_err(|e| e.to_string())?;
                validate(request, path, range)?;
                let text = item["contents"]
                    .as_str()
                    .ok_or("Hover contents must be text")?;
                batch.push((range, text.to_owned()));
            }
            Ok::<_, String>(batch)
        })();
        if let Ok(batch) = batch {
            for (range, text) in batch {
                if position >= range.start && position <= range.end {
                    return Some(Hover {
                        range: Some(range),
                        contents: HoverContents::Markup(MarkupContent {
                            kind: MarkupKind::Markdown,
                            value: text,
                        }),
                    });
                }
            }
        }
    }
    None
}
pub(crate) fn formatting(
    request: &RequestContext<'_>,
    path: &Path,
) -> Result<Vec<TextEdit>, String> {
    let doc = request
        .workspace()
        .documents
        .get(path)
        .ok_or("Unknown document")?;
    let mut edits = crate::tables::formatting(doc);
    for module in request
        .workspace()
        .modules
        .active()
        .filter(|m| m.kind == ModuleKind::Feature && m.has(Hook::Format))
    {
        for value in call(request, path, module, Hook::Format)? {
            edits.push(serde_json::from_value(value).map_err(|e| e.to_string())?);
        }
    }
    crate::actions::apply_edits(&doc.text, &edits)?;
    edits.sort_by_key(|e| (e.range.start, e.range.end));
    Ok(edits)
}
