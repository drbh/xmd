//! Explicit note dependencies and source navigation, using the expression parser.
use crate::{
    document::Span,
    engine::{Expr, Parser, Value},
    workspace::{Symbol, SymbolKind, Workspace},
};
use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct Member {
    pub source: String,
    pub span: Span,
}

pub(crate) fn is_note_path(id: &str) -> bool {
    id.starts_with('.') || id.contains('/') || id.ends_with(".wtf")
}

/// Normalize lexically so native and virtual files use identical import identities.
pub(crate) fn note_path(from: &Path, id: &str) -> Result<PathBuf, String> {
    if !(id.starts_with("./") || id.starts_with("../") || Path::new(id).is_absolute())
        || Path::new(id).extension().is_none_or(|s| s != "wtf")
        || id.contains(['\0', '\\'])
    {
        return Err("Use an explicit .wtf path, e.g. import(\"./values.wtf\")".into());
    }
    let joined = from.parent().unwrap_or(Path::new(".")).join(id);
    let mut path = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::CurDir => (),
            Component::ParentDir => {
                path.pop();
            }
            part => path.push(part.as_os_str()),
        }
    }
    Ok(path)
}

impl Expr {
    pub(crate) fn note_imports(&self) -> Vec<String> {
        let mut imports = Vec::new();
        self.walk(&mut |e| {
            if let Expr::Builtin(crate::engine::Builtin::Import, args) = e.bare()
                && let [arg] = args.as_slice()
                && let Expr::Value(Value::Text(id)) = arg.bare()
                && is_note_path(id)
            {
                imports.push(id.clone());
            }
        });
        imports
    }
    fn walk(&self, visit: &mut impl FnMut(&Expr)) {
        visit(self);
        match self.bare() {
            Self::Unary(_, e) | Self::Property(e, _) | Self::Lambda(_, e) => e.walk(visit),
            Self::Binary(_, a, b) => {
                a.walk(visit);
                b.walk(visit);
            }
            Self::Call(_, args) | Self::Builtin(_, args) | Self::List(args) => {
                args.iter().for_each(|e| e.walk(visit))
            }
            Self::Record(fields) => fields.iter().for_each(|(_, e)| e.walk(visit)),
            Self::Apply(f, args) => {
                f.walk(visit);
                args.iter().for_each(|e| e.walk(visit));
            }
            _ => (),
        }
    }
}

pub(crate) fn analyze(source: &str, text: &str, span: Span) -> (Vec<String>, Vec<Member>) {
    let Ok(expr) = Parser::parse(source) else {
        return Default::default();
    };
    let free: BTreeSet<_> = expr.free_names().into_iter().map(|(_, at)| at).collect();
    let imports = expr.note_imports();
    let tokens = crate::engine::lex(source).unwrap_or_default();
    let mut members = Vec::new();
    expr.walk(&mut |e| {
        if let Expr::Property(_, key) = e.bare() {
            let (start, end) = e.bounds();
            // A function parameter's properties do not refer to note definitions.
            if e.free_names().iter().all(|(_, at)| free.contains(at))
                && let Some(token) = tokens.iter().rev().find(|t| {
                    t.start >= start
                        && t.end <= end
                        && matches!(&t.kind, crate::engine::Lexeme::Name(name) if name == key)
                })
            {
                members.push(Member {
                    source: source[start..end].into(),
                    span: span.relative(text, token.start, token.end),
                });
            }
        }
    });
    (imports, members)
}

/// Resolve static namespace accesses without evaluating user code or reading files.
pub(crate) fn member_symbol(ws: &Workspace, path: &Path, source: &str) -> Option<Symbol> {
    fn target(
        ws: &Workspace,
        path: &Path,
        expr: &Expr,
        seen: &mut BTreeSet<Symbol>,
    ) -> Option<Symbol> {
        match expr.bare() {
            Expr::Name(name) => ws.resolve(path, name).ok(),
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
            && let Expr::Value(Value::Text(id)) = arg.bare()
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
