//! Resolving an import expression against a live workspace. What an
//! expression *says* about its dependencies — whether it names a note import,
//! and which of its properties read a member — is `model::imports`, one
//! layer down; this walks that same syntax against the workspace's symbols.
use crate::{
    engine::{Expr, Parser},
    workspace::{Symbol, SymbolKind, Workspace},
};
use model::note_path;
use std::{collections::BTreeSet, path::Path, path::PathBuf};
use syntax::Literal;

/// Resolve static namespace accesses without evaluating user code or reading files.
pub fn member_symbol(ws: &Workspace, path: &Path, source: &str) -> Option<Symbol> {
    fn target(
        ws: &Workspace,
        path: &Path,
        expr: &Expr,
        seen: &mut BTreeSet<Symbol>,
    ) -> Option<Symbol> {
        match expr.bare() {
            Expr::Name(name) | Expr::Param { name, .. } => ws.resolve(path, name).ok(),
            Expr::Property(receiver, key) => {
                ws.resolve(&namespace(ws, path, receiver, seen)?, key).ok()
            }
            _ => None,
        }
    }
    fn namespace(
        ws: &Workspace,
        path: &Path,
        expr: &Expr,
        seen: &mut BTreeSet<Symbol>,
    ) -> Option<PathBuf> {
        if let Expr::Builtin(crate::engine::Builtin::Import, args) = expr.bare()
            && let [arg] = args.as_slice()
            && let Expr::Value(Literal::Text(id)) = arg.bare()
        {
            let path = note_path(path, id).ok()?;
            return ws.documents.contains_key(&path).then_some(path);
        }
        let symbol = target(ws, path, expr, seen)?;
        if seen.len() >= 64 || !seen.insert(symbol.clone()) {
            return None;
        }
        let SymbolKind::Definition(i) = symbol.kind else {
            return None;
        };
        let def = &ws.documents[&symbol.path].definitions[i];
        namespace(ws, &symbol.path, &Parser::parse(&def.source).ok()?, seen)
    }
    target(ws, path, &Parser::parse(source).ok()?, &mut BTreeSet::new())
}
