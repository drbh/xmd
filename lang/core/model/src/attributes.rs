//! What a line's `@key(value)` attributes mean, from the attribute table in
//! `syntax`: how each value is painted (a date, a time stamp, an expression
//! or text) and the problems of a repeated, unknown or unclosed attribute.
//! The tasks and events recognizers read the attributes this one checks.
use crate::blocks::{HighlightKind, Line, Problem, Tree};
use crate::document::Document;
use syntax::{AttributeKey, AttributeValue};

pub(crate) fn recognize(tree: &mut Tree, _: &mut Document, line: &Line<'_>) {
    let row = line.row;
    for (index, (key, attr)) in line.attributes.list.iter().enumerate() {
        let value = attr.value_span;
        // What the value holds decides how it is painted: a date that reads
        // as one without evaluating, an expression, or plain text.
        let known = key.parse::<AttributeKey>().ok();
        match known.map(AttributeKey::value) {
            Some(AttributeValue::When) if syntax::is_relative_date(&attr.value) => {
                tree.mark(row, value.start, value.end, HighlightKind::Number);
            }
            Some(AttributeValue::Stamp) if syntax::stamp(&attr.value).is_some() => {
                tree.mark(row, value.start, value.end, HighlightKind::Number);
            }
            Some(kind) if kind.is_expression() => {
                tree.expression(line.text, row, value.start, value.end);
            }
            _ => tree.mark(row, value.start, value.end, HighlightKind::String),
        }
        if line.attributes.list[..index].iter().any(|(k, _)| k == key) {
            tree.problems.push(Problem {
                span: attr.span,
                message: format!("Duplicate @{key} attribute"),
            });
        }
        if known.is_none() {
            tree.problems.push(Problem {
                span: attr.span,
                message: format!("Unknown attribute @{key}"),
            });
        }
    }
    if let Some(span) = line.attributes.unclosed {
        tree.problems.push(Problem {
            span,
            message: "Unclosed task attribute".into(),
        });
    }
}
