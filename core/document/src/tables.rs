//! Explicit computational tables. Parsing and typing are shared
//! language features, not browser-side Markdown interpretation. Reading one
//! against a live workspace — resolving a column reference, renaming a
//! symbol, or computing a `TableValue` a note holds — is `evaluate::tables`,
//! one layer up.
use crate::blocks::{Definition, HighlightKind, Link, Named, Problem, Tree, cells, identifier};
use crate::document::Document;
use common::Span;
use common::ValueType;
use syntax::Literal;

#[derive(Clone, Debug)]
pub struct Cell {
    pub source: String,
    pub span: Span,
    pub value: Result<Literal, String>,
    /// `[name]` or `[a * b]`: a calculation evaluated with the table, so cells
    /// can read local names and explicitly imported values.
    pub expression: Option<(String, Span)>,
}
impl Cell {
    pub fn calculated(&self) -> bool {
        self.expression.is_some()
    }
}
/// What a decision column holds for each row: an unknown a linear reading of
/// a `sum` over the table solves for (a plan chooses it), never a value a
/// note writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Domain {
    /// `name?`: yes or no.
    Choice,
    /// `name#`: a whole number, at least zero.
    Count,
}
impl Domain {
    /// The column type a decision column reports, like any other column.
    pub fn value_type(self) -> ValueType {
        match self {
            Self::Choice => ValueType::Choice,
            Self::Count => ValueType::Count,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Table {
    pub definition: usize,
    pub header: usize,
    pub end_line: usize,
    pub columns: Vec<Named>,
    pub rows: Vec<Vec<Cell>>,
    pub types: Vec<Option<ValueType>>,
    pub separators: Vec<String>,
    pub problems: Vec<Problem>,
    /// One entry per column; `Some` marks a decision column, which a form
    /// that sums over the table solves for.
    pub domains: Vec<Option<Domain>>,
}

/// `name := table` on `row` and the markdown table under it: its columns,
/// and every cell's literal, formula or link. Returns how many rows below
/// `row` the table took.
pub(crate) fn recognize(
    tree: &mut Tree,
    doc: &mut Document,
    lines: &[&str],
    row: usize,
) -> Option<usize> {
    let index = tree.opened(row)?;
    if tree.definitions[index].source != "table" {
        return None;
    }
    let table = parse(&tree.definitions[index], index, lines);
    let end_line = table.end_line;
    // The declaration keyword isn't a global reference.
    tree.references
        .retain(|r| !(r.span.line == row && r.name == "table"));
    for column in &table.columns {
        tree.paint(column.span, HighlightKind::Variable);
    }
    for cells in &table.rows {
        for cell in cells {
            let Span { line, start, end } = cell.span;
            if let Some((_, span)) = &cell.expression {
                tree.mark(line, start, start + 1, HighlightKind::Operator);
                tree.mark(line, end - 1, end, HighlightKind::Operator);
                tree.expression(lines[span.line], span.line, span.start, span.end);
                continue;
            }
            if let Ok(Literal::Resource(resource)) = &cell.value {
                tree.links.push(Link {
                    span: cell.span,
                    target: resource.target.clone(),
                });
            }
            tree.paint(
                cell.span,
                if matches!(&cell.value, Ok(Literal::Text(_) | Literal::Resource(_))) {
                    HighlightKind::String
                } else {
                    HighlightKind::Number
                },
            );
        }
    }
    tree.problems.extend(table.problems.clone());
    doc.tables.push(table);
    Some(end_line.saturating_sub(row + 1))
}

fn parse(def: &Definition, definition: usize, lines: &[&str]) -> Table {
    let header = def.end.line + 1;
    let mut table = Table {
        definition,
        header,
        end_line: header,
        columns: vec![],
        rows: vec![],
        types: vec![],
        separators: vec![],
        problems: vec![],
        domains: vec![],
    };
    let mut problem = |span, message| table.problems.push(Problem { span, message });
    let Some(headers) = lines.get(header).and_then(|l| cells(l, header)) else {
        problem(
            def.value_span,
            "A table needs a pipe-delimited header on the next line".into(),
        );
        return table;
    };
    for (raw, span) in headers {
        let domain = match raw.chars().last() {
            Some('?') => Some(Domain::Choice),
            Some('#') => Some(Domain::Count),
            _ => None,
        };
        let name = raw[..raw.len() - usize::from(domain.is_some())].to_string();
        let span = Span::new(span.line, span.start, span.start + name.len());
        if !identifier(&name) || matches!(name.as_str(), "true" | "false") {
            problem(
                span,
                "Column names must be identifiers (not true or false)".into(),
            );
        }
        if table.columns.iter().any(|c: &Named| c.name == name) {
            problem(span, format!("Duplicate column '{name}'"));
        }
        table.columns.push(Named { name, span });
        table.domains.push(domain);
    }
    table.end_line = header + 1;
    let separator = lines.get(header + 1).and_then(|l| cells(l, header + 1));
    if let Some(parts) = separator {
        table.end_line = header + 2;
        table.separators = parts.iter().map(|(s, _)| s.clone()).collect();
        if parts.len() != table.columns.len()
            || parts.iter().any(|(s, _)| {
                let core = s.strip_prefix(':').unwrap_or(s);
                let core = core.strip_suffix(':').unwrap_or(core);
                core.len() < 3 || !core.bytes().all(|c| c == b'-')
            })
        {
            problem(
                Span::new(header + 1, 0, lines[header + 1].len()),
                "Table separator must have one --- cell per column".into(),
            );
        }
    } else {
        problem(
            def.value_span,
            "A table needs a Markdown separator row after its header".into(),
        );
    }
    while let Some(line) = lines
        .get(table.end_line)
        .filter(|l| l.trim_start().starts_with('|'))
    {
        let row = table.end_line;
        table.end_line += 1;
        let Some(parts) = cells(line, row) else {
            problem(
                Span::new(row, 0, line.len()),
                "Unclosed table row or quoted cell; use outer | delimiters".into(),
            );
            continue;
        };
        if parts.len() != table.columns.len() {
            // A calculation cell split in two was most likely a pipe.
            let split = parts.iter().any(|(cell, _)| {
                cell.starts_with('[') && cell.matches('[').count() > cell.matches(']').count()
            });
            problem(
                Span::new(row, 0, line.len()),
                format!(
                    "Expected {} cells, found {}{}",
                    table.columns.len(),
                    parts.len(),
                    if split {
                        "; a | inside [ ] is written \\| in a table"
                    } else {
                        ""
                    }
                ),
            );
        }
        let decision = |column| table.domains.get(column).is_some_and(Option::is_some);
        let cells = parts.into_iter().enumerate();
        let cells = cells.map(|(column, (source, span))| cell(source, span, decision(column)));
        table.rows.push(cells.collect());
    }
    table.types = table
        .domains
        .iter()
        .map(|d| d.map(Domain::value_type))
        .collect();
    for row in &table.rows {
        for (column, cell) in row.iter().enumerate().take(table.columns.len()) {
            if table.domains[column].is_some()
                || cell
                    .expression
                    .as_ref()
                    .is_some_and(|(inner, _)| !inner.is_empty() && syntax::valid_expression(inner))
            {
                continue;
            }
            match &cell.value {
                Err(message) => problem(cell.span, message.clone()),
                Ok(value) => match (table.types[column], literal_kind(value)) {
                    (None, kind) => table.types[column] = Some(kind),
                    (Some(expected), kind) if expected != kind => problem(
                        cell.span,
                        format!(
                            "Column '{}' expects {expected}, found {kind}",
                            table.columns[column].name
                        ),
                    ),
                    _ => {}
                },
            }
        }
    }
    table
}
/// A row's cell as written: a `[calculation]` evaluated with the table, or a
/// literal, read here. A decision column's cell is never either.
fn cell(source: String, span: Span, decision: bool) -> Cell {
    let decoded = source.replace("\\|", "|");
    let expression = decoded
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .filter(|_| !decision)
        .map(|inner| {
            let start = span.start + 1 + inner.len() - inner.trim_start().len();
            let inner = inner.trim();
            let at = Span::new(span.line, start, start + inner.len());
            (inner.to_string(), at)
        });
    let value = match &expression {
        Some((inner, _)) if inner.is_empty() => {
            Err("Empty calculation; write a name or expression inside the brackets".to_string())
        }
        Some((inner, _)) if syntax::valid_expression(inner) => Err(format!(
            "Calculated cell [{inner}] is evaluated with the table"
        )),
        Some((inner, _)) => Err(format!("Invalid calculation '{inner}'")),
        // A plan decides these; whatever is written is a note to self.
        None if decision => Ok(Literal::Text(decoded.clone())),
        None if decoded.is_empty() => Err("Missing cell value".to_string()),
        None => syntax::literal(&decoded).and_then(|v| {
            let numeric = decoded.starts_with(|c: char| c.is_ascii_digit() || "$-+".contains(c));
            if matches!(v, Literal::Text(_)) && numeric {
                Err(format!(
                    "Invalid scalar literal '{decoded}'; quote it to store text"
                ))
            } else {
                Ok(v)
            }
        }),
    };
    Cell {
        source,
        span,
        value,
        expression,
    }
}
/// The [`ValueType`] a parsed literal reports, mirroring `Value::kind` for
/// the scalar shapes a table cell can hold.
fn literal_kind(value: &Literal) -> ValueType {
    match value {
        Literal::Resource(_) => ValueType::Resource,
        Literal::Date(_) => ValueType::Date,
        Literal::DateTime(_) => ValueType::DateTime,
        Literal::Duration(_) => ValueType::Duration,
        Literal::Bool(_) => ValueType::Boolean,
        Literal::Money(..) => ValueType::Money,
        Literal::Ratio(_) => ValueType::Ratio,
        Literal::Number(_) => ValueType::Number,
        Literal::Text(_) => ValueType::Text,
    }
}

/// Find the innermost sum row scope, including incomplete formulas while typing.
pub fn scope_at(doc: &Document, span: Span) -> Option<String> {
    doc.sums.iter().find_map(|region| {
        let offset = region.offset_of(doc, span)?;
        syntax::sum_scope_at(region.source(doc), offset)
    })
}
/// Data tables plus the tables forms take, which share the same grid shape.
pub fn grids(doc: &Document) -> Vec<Table> {
    doc.tables
        .iter()
        .cloned()
        .chain(
            doc.forms
                .iter()
                .filter(|f| f.has_table())
                .map(crate::forms_impl::grid),
        )
        .collect()
}
