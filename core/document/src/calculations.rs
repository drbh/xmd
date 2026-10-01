//! Line calculations: a line that is only math, with its variables in
//! brackets, shows its result at the end: `[budget] - [spent]`. Bracketed
//! calculations in prose (`[a / b]`) are an inline form the generic layer reads.
use crate::blocks::Line;
use crate::document::Document;
use crate::inline::{Calculation, bracket_reference};
use crate::tree::Tree;
use common::Span;

pub(crate) fn recognize(tree: &mut Tree, _: &mut Document, line: &Line<'_>) {
    if line.checkbox.is_some() || !line.attributes.list.is_empty() {
        return;
    }
    let (start, row) = (line.start, line.row);
    let trimmed = &line.text[start..];
    if let Some(source) = line_calculation(trimmed) {
        tree.calculations.push(Calculation {
            span: Span::new(row, start, start + trimmed.trim_end().len()),
            source,
            bracketed: false,
        });
    }
}

/// The expression for a line of math such as `[budget] - [spent] * 2` or
/// `2 + 2`: brackets around names become spaces, so offsets line up with the
/// line. Prose, list items, lone values, and bare names are not lines of math.
fn line_calculation(trimmed: &str) -> Option<String> {
    let trimmed = trimmed.trim_end();
    if trimmed.is_empty()
        || trimmed.starts_with(['-', '*', '+', '>', '#', '|', '`', '<', '!'])
        || trimmed.contains("](")
    {
        return None;
    }
    let mut masked = String::with_capacity(trimmed.len());
    let mut names: Vec<(usize, usize)> = Vec::new();
    let mut rest = trimmed;
    let mut at = 0;
    while let Some(open) = rest.find('[') {
        let close = rest[open..].find(']')? + open;
        bracket_reference(rest[open + 1..close].trim())?;
        masked.push_str(&rest[..open]);
        masked.push(' ');
        masked.push_str(&rest[open + 1..close]);
        masked.push(' ');
        names.push((at + open + 1, at + close));
        at += close + 1;
        rest = &rest[close + 1..];
    }
    masked.push_str(rest);
    let tokens = syntax::lex(&masked).ok()?;
    let mut meaningful = false;
    for (i, token) in tokens.iter().enumerate() {
        match &token.kind {
            syntax::Lexeme::Name(n) => {
                let call = matches!(
                    tokens.get(i + 1).map(|t| &t.kind),
                    Some(syntax::Lexeme::Left)
                );
                if call {
                    meaningful = true;
                } else if !names
                    .iter()
                    .any(|(s, e)| token.start >= *s && token.end <= *e)
                    && !common::is_code(n)
                    && !matches!(n.as_str(), "true" | "false")
                {
                    // A bare word is prose, not a variable.
                    return None;
                }
            }
            syntax::Lexeme::Op(_) => meaningful = true,
            _ => {}
        }
    }
    (meaningful && syntax::valid_expression(&masked)).then_some(masked)
}
