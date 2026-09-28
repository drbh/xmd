//! Navigation: finding the symbol under a position, every place it occurs, and
//! the spans where an editor should stay quiet. Hovers, completions and
//! signature help live beside this module and are re-exported here, so
//! `wtf::intelligence` still names the whole editor-intelligence surface.
use common::Span;
use eval::{Symbol, SymbolKind, Workspace};
use lsp_types::*;
use model::{Document, byte_at};
use std::path::Path;

pub use crate::signature::signature;
pub(crate) use crate::{
    completion::completions,
    hover::{calculation_hover, cell_hover, hover_at, link_hover, symbol_hover},
    signature::is_builtin_function,
};

pub fn symbol_at(workspace: &Workspace, path: &Path, position: Position) -> Option<(Symbol, Span)> {
    let doc = workspace.documents.get(path)?;
    let byte = byte_at(doc.line(position.line as usize), position.character)?;
    let inside =
        |span: Span| span.line == position.line as usize && byte >= span.start && byte <= span.end;
    if let Some(member) = doc.members.iter().find(|m| inside(m.span))
        && let Some(symbol) = eval::member_symbol(workspace, path, &member.source)
    {
        return Some((symbol, member.span));
    }
    for (table, t) in doc.tables.iter().enumerate() {
        for (column, c) in t.columns.iter().enumerate() {
            if inside(c.span) {
                return Some((
                    Symbol {
                        path: path.into(),
                        kind: SymbolKind::Column(table, column),
                    },
                    c.span,
                ));
            }
        }
    }
    for symbol in workspace.symbols().into_iter().filter(|s| s.path == path) {
        let span = workspace.named(&symbol).span;
        if inside(span) {
            return Some((symbol, span));
        }
    }
    doc.references
        .iter()
        .find(|r| inside(Span::new(r.span.line, r.span.start, r.end())))
        .and_then(|r| {
            eval::tables::resolve_reference(workspace, path, r)
                .ok()
                .map(|s| (s, r.span))
        })
}
/// Every place a symbol appears: its declaration, references that resolve to
/// it, and for decision variables the `plan.variable` property accesses.
/// Sorted by note and position, without duplicates.
pub fn occurrences(ws: &Workspace, symbol: &Symbol) -> Vec<(std::path::PathBuf, Span)> {
    let mut found = vec![(symbol.path.clone(), ws.named(symbol).span)];
    let plan = match symbol.kind {
        SymbolKind::Variable(p, _) => Some(Symbol {
            path: symbol.path.clone(),
            kind: SymbolKind::Definition(ws.documents[&symbol.path].plans[p].definition),
        }),
        _ => None,
    };
    let name = &ws.named(symbol).name;
    for (path, doc) in &ws.documents {
        for member in &doc.members {
            if eval::member_symbol(ws, path, &member.source).as_ref() == Some(symbol) {
                found.push((path.clone(), member.span));
            }
        }
        for r in &doc.references {
            if r.name == *name
                && eval::tables::resolve_reference(ws, path, r).ok().as_ref() == Some(symbol)
            {
                found.push((path.clone(), r.span));
            }
            if let (Some(plan), Some(property)) = (&plan, &r.property)
                && property == name
                && ws.resolve(path, &r.name).ok().as_ref() == Some(plan)
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

pub(crate) fn inert(doc: &Document, position: Position) -> bool {
    let row = position.line as usize;
    let Some(byte) = byte_at(doc.line(row), position.character) else {
        return true;
    };
    doc.highlights.iter().any(|h| {
        h.span.line == row
            && byte >= h.span.start
            && byte < h.span.end
            && (h.kind == model::HighlightKind::Comment
                || h.kind == model::HighlightKind::String
                    && !doc
                        .tasks
                        .iter()
                        .flat_map(|t| t.attributes.values())
                        .chain(doc.events.iter().flat_map(|e| e.attributes.values()))
                        .any(|a| a.span.line == row && byte >= a.span.start && byte <= a.span.end)
                    && !doc.definitions.iter().any(|d| {
                        d.expression && d.value_span.contains(&doc.text, Span::new(row, byte, byte))
                    }))
    })
}
