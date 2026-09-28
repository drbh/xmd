//! Every place a symbol occurs across the workspace, for references, rename
//! and highlights.
use lang::common::Span;
use lang::eval::{Symbol, SymbolKind, Workspace};

/// Every place a symbol appears: its declaration, references that resolve to
/// it, and for decision variables the `plan.variable` property accesses.
/// Sorted by note and position, without duplicates.
pub fn occurrences(ws: &Workspace, symbol: &Symbol) -> Vec<(std::path::PathBuf, Span)> {
    let mut found = vec![(symbol.path.clone(), ws.named(symbol).span)];
    let plan = match symbol.kind {
        SymbolKind::Variable(p, _) => Some(symbol.sibling(SymbolKind::Definition(
            ws.documents[&symbol.path].plans[p].definition,
        ))),
        _ => None,
    };
    let name = &ws.named(symbol).name;
    for (path, doc) in &ws.documents {
        for member in &doc.members {
            if lang::eval::member_symbol(ws, path, &member.source).as_ref() == Some(symbol) {
                found.push((path.clone(), member.span));
            }
        }
        for r in &doc.references {
            if r.name == *name
                && lang::eval::tables::resolve_reference(ws, path, r)
                    .ok()
                    .as_ref()
                    == Some(symbol)
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
