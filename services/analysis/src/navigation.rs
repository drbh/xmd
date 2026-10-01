//! Every place a symbol occurs across the workspace, and what definition,
//! references, highlights and rename make of them, the same on every host.
use lang::common::{Span, uri, uri_from_url};
use lang::eval::{Symbol, SymbolKind, Workspace};
use lsp_types::{DocumentHighlight, DocumentHighlightKind, Location, TextEdit};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Every place a symbol appears: its declaration, references that resolve to
/// it, and for a name a form solves for the `definition.name` property
/// accesses.
/// Sorted by note and position, without duplicates.
fn occurrences(ws: &Workspace, symbol: &Symbol) -> Vec<(PathBuf, Span)> {
    let mut found = vec![(symbol.path.clone(), ws.named(symbol).span)];
    let definition = match symbol.kind {
        SymbolKind::Variable(f, _) => Some(symbol.sibling(SymbolKind::Definition(
            ws.documents()[&symbol.path].forms()[f].definition,
        ))),
        _ => None,
    };
    let name = &ws.named(symbol).name;
    for (path, doc) in ws.documents() {
        for member in doc.members() {
            if lang::eval::member_symbol(ws, path, &member.source).as_ref() == Some(symbol) {
                found.push((path.clone(), member.span));
            }
        }
        for r in doc.references() {
            if r.name == *name
                && lang::eval::tables::resolve_reference(ws, path, r)
                    .ok()
                    .as_ref()
                    == Some(symbol)
            {
                found.push((path.clone(), r.span));
            }
            if let (Some(definition), Some(property)) = (&definition, &r.property)
                && property == name
                && ws.resolve(path, &r.name).ok().as_ref() == Some(definition)
            {
                found.push((
                    path.clone(),
                    Span::new(r.span.line, r.span.end + 1, r.end()),
                ));
            }
        }
    }
    found.sort_by_key(|(p, s)| (p.clone(), s.line, s.start, s.end));
    found.dedup();
    // The declaration stays first so callers can treat it as the write site.
    let declaration = (symbol.path.clone(), ws.named(symbol).span);
    found.retain(|o| *o != declaration);
    found.insert(0, declaration);
    found
}

/// `span` in the note at `path`, as a location a client opens.
fn location(ws: &Workspace, path: &Path, span: Span) -> Location {
    Location {
        uri: uri_from_url(&uri(path)),
        range: span.range(&ws.documents()[path]),
    }
}
/// Where `symbol` is declared.
pub fn definition(ws: &Workspace, symbol: &Symbol) -> Location {
    location(ws, &symbol.path, ws.named(symbol).span)
}
/// Every occurrence of `symbol`, its declaration first.
pub fn references(ws: &Workspace, symbol: &Symbol) -> Vec<Location> {
    occurrences(ws, symbol)
        .into_iter()
        .map(|(path, span)| location(ws, &path, span))
        .collect()
}
/// The occurrences of `symbol` in the note at `path`: its declaration is
/// written, the others read.
pub fn highlights(ws: &Workspace, path: &Path, symbol: &Symbol) -> Vec<DocumentHighlight> {
    occurrences(ws, symbol)
        .into_iter()
        .enumerate()
        .filter(|(_, (p, _))| p == path)
        .map(|(i, (_, span))| DocumentHighlight {
            range: span.range(&ws.documents()[path]),
            kind: Some(if i == 0 {
                DocumentHighlightKind::WRITE
            } else {
                DocumentHighlightKind::READ
            }),
        })
        .collect()
}
/// The edits renaming `symbol` to `name` makes, by note, once the new name
/// is free everywhere it is read.
pub fn rename(
    ws: &Workspace,
    symbol: &Symbol,
    name: &str,
) -> Result<BTreeMap<PathBuf, Vec<TextEdit>>, String> {
    lang::eval::tables::validate_rename(ws, symbol, name)?;
    let mut changes: BTreeMap<PathBuf, Vec<TextEdit>> = BTreeMap::new();
    for (path, span) in occurrences(ws, symbol) {
        let range = span.range(&ws.documents()[&path]);
        changes
            .entry(path)
            .or_default()
            .push(TextEdit::new(range, name.into()));
    }
    Ok(changes)
}
