//! An expression read back for its reader: the worked step a hover shows,
//! each name replaced by its value (`substituted`), and whether a selection
//! is one whole subexpression, which extracting it needs
//! (`is_subexpression`).
use crate::engine::{Engine, Expr, Lexeme, Parser, Value, ValueType, keyword, lex, sum_scope_at};
use std::path::Path;
use values::{EvalError, EvalResult};

impl Engine<'_> {
    pub fn is_subexpression(source: &str, start: usize, end: usize) -> bool {
        let Ok(expr) = Parser::parse(source) else {
            return false;
        };
        // Every node, each span around one included.
        let mut found = false;
        expr.walk(&mut |mut node| {
            loop {
                found |= node.bounds() == (start, end);
                let Expr::Spanned(_, _, inner) = node else {
                    break;
                };
                node = inner;
            }
        });
        found
    }
    /// Return a substitution trace without re-evaluating side effects (evaluation is pure).
    pub fn substituted(&mut self, path: &Path, source: &str) -> EvalResult<String> {
        let tokens = lex(source).map_err(EvalError::Message)?;
        let mut edits = Vec::new();
        for (i, token) in tokens.iter().enumerate() {
            let next = tokens.get(i + 1).map(|t| &t.kind);
            if let Lexeme::Name(name) = &token.kind
                && !matches!(next, Some(Lexeme::Left))
                && (i == 0 || !matches!(tokens[i - 1].kind, Lexeme::Dot))
                && !matches!(keyword(name), Some(Value::Bool(_)))
                && sum_scope_at(source, token.start).is_none()
                && let Ok(value) = self.named(path, name)
            {
                if value.kind() == ValueType::Table {
                    continue;
                }
                // For properties substitute the complete access, not an object's display text.
                let end = if matches!(next, Some(Lexeme::Dot)) {
                    tokens.get(i + 2).map(|t| t.end).unwrap_or(token.end)
                } else {
                    token.end
                };
                let value = if end > token.end {
                    self.eval(path, &source[token.start..end])?
                } else {
                    value
                };
                edits.push((token.start, end, value.display()));
            }
        }
        let mut result = source.to_string();
        for (start, end, value) in edits.into_iter().rev() {
            result.replace_range(start..end, &value);
        }
        Ok(result)
    }
}
