//! Explicit computational tables. Parsing and typing are shared
//! language features, not browser-side Markdown interpretation. Reading one
//! against a live workspace — resolving a column reference, renaming a
//! symbol, or computing a `TableValue` a note holds — is `evaluate::tables`,
//! one layer up.
use crate::blocks::cells;
use crate::document::Document;
use crate::inline::{Definition, Link, Named, identifier};
use crate::tree::{HighlightKind, Problem, Tree};
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
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
                tree.links
                    .push(Link::new(cell.span, resource.target.clone()));
            }
            let text = matches!(&cell.value, Ok(Literal::Text(_) | Literal::Resource(_)));
            tree.paint(cell.span, HighlightKind::literal(text));
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
        let message = "A table needs a pipe-delimited header on the next line";
        problem(def.value_span, message.into());
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
            let message = "Column names must be identifiers (not true or false)";
            problem(span, message.into());
        }
        if table.columns.iter().any(|c: &Named| c.name == name) {
            problem(span, format!("Duplicate column '{name}'"));
        }
        table.columns.push(Named { name, span });
        table.domains.push(domain);
    }
    let message = "A table needs a Markdown separator row after its header";
    let (missing, width) = ((def.value_span, message.into()), table.columns.len());
    let (separators, end_line, grid) = rows(lines, header, width, false, missing, &mut problem);
    (table.separators, table.end_line) = (separators, end_line);
    for (row, line, parts) in grid {
        let Some(parts) = parts else {
            let message = "Unclosed table row or quoted cell; use outer | delimiters";
            problem(Span::new(row, 0, line.len()), message.into());
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
/// What follows a grid's header on line `header`: its separator row, checked
/// against `width` columns (with `colons`, any run of alignment colons at
/// either end, rather than one), or the problem `missing` names; then each
/// `|` row with its cells, or `None` for one that does not close. Returns
/// the separator's cells and one past the last row with them.
pub(crate) fn rows<'l>(
    lines: &[&'l str],
    header: usize,
    width: usize,
    colons: bool,
    missing: (Span, String),
    problem: &mut impl FnMut(Span, String),
) -> (Vec<String>, usize, Vec<Row<'l>>) {
    let (mut separators, mut end_line, mut rows) = (vec![], header + 1, vec![]);
    if let Some(parts) = lines.get(header + 1).and_then(|l| cells(l, header + 1)) {
        end_line = header + 2;
        separators = parts.iter().map(|(s, _)| s.clone()).collect();
        let dashes = |s: &str| {
            let core = s.strip_prefix(':').unwrap_or(s);
            let core = core.strip_suffix(':').unwrap_or(core);
            let core = core.trim_matches(|c| colons && c == ':');
            core.len() < 3 || !core.bytes().all(|c| c == b'-')
        };
        if parts.len() != width || parts.iter().any(|(s, _)| dashes(s)) {
            let at = Span::new(header + 1, 0, lines[header + 1].len());
            let message = "Table separator must have one --- cell per column";
            problem(at, message.into());
        }
    } else {
        problem(missing.0, missing.1);
    }
    while let Some(line) = lines
        .get(end_line)
        .filter(|l| l.trim_start().starts_with('|'))
    {
        rows.push((end_line, *line, cells(line, end_line)));
        end_line += 1;
    }
    (separators, end_line, rows)
}
/// A grid row: its line, its text, and its cells unless it does not close.
pub(crate) type Row<'l> = (usize, &'l str, Option<Vec<(String, Span)>>);
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
    let forms = doc.forms.iter().filter(|f| f.has_table());
    let forms = forms.map(crate::forms_impl::grid);
    doc.tables.iter().cloned().chain(forms).collect()
}
