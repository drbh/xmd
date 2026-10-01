//! Explicit note dependencies and source navigation, using the expression parser.
use common::Span;
use std::{
    collections::BTreeSet,
    path::{Component, Path, PathBuf},
};
use syntax::{Expr, Literal, Parser};

#[derive(Clone, Debug)]
pub struct Member {
    pub source: String,
    pub span: Span,
}

pub fn is_note_path(id: &str) -> bool {
    id.starts_with('.') || id.contains('/') || common::is_note(id)
}

/// Normalize lexically so native and virtual files use identical import identities.
pub fn note_path(from: &Path, id: &str) -> Result<PathBuf, String> {
    if !(id.starts_with("./") || id.starts_with("../") || Path::new(id).is_absolute())
        || !common::is_note(id)
        || id.contains(['\0', '\\'])
    {
        return Err(format!(
            "Use an explicit .{} or .{} path, e.g. import(\"./{}\")",
            common::EXTENSION,
            common::LIBRARY_EXTENSION,
            common::note_file("values")
        ));
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

/// `Expr::note_imports` needs `is_note_path`, which is `document`'s, so it is an
/// extension trait here rather than an inherent impl on syntax's `Expr`.
pub trait ExprImports {
    fn note_imports(&self) -> Vec<String>;
}
impl ExprImports for Expr {
    fn note_imports(&self) -> Vec<String> {
        let mut imports = Vec::new();
        self.walk(&mut |e| {
            if let Expr::Builtin(syntax::Builtin::Import, args) = e.bare()
                && let [arg] = args.as_slice()
                && let Expr::Value(Literal::Text(id)) = arg.bare()
                && is_note_path(id)
            {
                imports.push(id.clone());
            }
        });
        imports
    }
}

pub(crate) fn analyze(source: &str, text: &str, span: Span) -> (Vec<String>, Vec<Member>) {
    let Ok(expr) = Parser::parse(source) else {
        return Default::default();
    };
    let free: BTreeSet<_> = expr.free_names().into_iter().map(|(_, at)| at).collect();
    let imports = expr.note_imports();
    let tokens = syntax::lex(source).unwrap_or_default();
    let mut members = Vec::new();
    expr.walk(&mut |e| {
        if let Expr::Property(_, key) = e.bare() {
            let (start, end) = e.bounds();
            // A function parameter's properties do not refer to note definitions.
            if !e.mentions_parameter()
                && e.free_names().iter().all(|(_, at)| free.contains(at))
                && let Some(token) = tokens.iter().rev().find(|t| {
                    t.start >= start
                        && t.end <= end
                        && matches!(&t.kind, syntax::Lexeme::Name(name) if name == key)
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
