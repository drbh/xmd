//! Explicit computational tables. Parsing, typing and formatting are shared
//! language features, not browser-side Markdown interpretation. Reading one
//! against a live workspace — resolving a column reference, renaming a
//! symbol, or computing a `TableValue` a note holds — is `evaluate::tables`,
//! one layer up.
use crate::document::{Document, Named, Problem, identifier};
use common::Span;
use common::ValueType;
use lsp_types::{Range, TextEdit};
use syntax::Literal;
use unicode_width::UnicodeWidthStr;

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
/// What a plan may choose for each row of a decision column.
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
    /// One entry per column; `Some` marks a decision column a plan fills in.
    pub domains: Vec<Option<Domain>>,
}

/// Pipes in quoted strings and escaped pipes are cell contents, not separators.
pub fn cells(line: &str, row: usize) -> Option<Vec<(String, Span)>> {
    let start = line.len() - line.trim_start().len();
    let end = line.trim_end().len();
    if !line[start..].starts_with('|') || end <= start + 1 {
        return None;
    }
    let mut quoted = false;
    let mut escaped = false;
    let mut last = start + 1;
    let mut result = Vec::new();
    for (i, c) in line.char_indices().filter(|(i, _)| *i > start && *i < end) {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
            continue;
        }
        if c == '"' {
            quoted = !quoted;
        }
        if c == '|' && !quoted {
            let raw = &line[last..i];
            let from = last + raw.len() - raw.trim_start().len();
            let to = from + raw.trim().len();
            result.push((raw.trim().into(), Span::new(row, from, to)));
            last = i + 1;
        }
    }
    (last == end && !quoted).then_some(result)
}

pub(crate) fn parse(doc: &Document, definition: usize, lines: &[&str]) -> Table {
    let def = &doc.definitions[definition];
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
        let domains = table.domains.clone();
        table.rows.push(
            parts
                .into_iter()
                .enumerate()
                .map(|(column, (source, span))| {
                    let decoded = source.replace("\\|", "|");
                    let expression = decoded
                        .strip_prefix('[')
                        .and_then(|s| s.strip_suffix(']'))
                        .filter(|_| domains.get(column).is_none_or(Option::is_none))
                        .map(|inner| {
                            let lead = 1 + inner.len() - inner.trim_start().len();
                            (
                                inner.trim().to_string(),
                                Span::new(
                                    span.line,
                                    span.start + lead,
                                    span.start + lead + inner.trim().len(),
                                ),
                            )
                        });
                    let value = if let Some((inner, _)) = &expression {
                        if inner.is_empty() {
                            Err(
                                "Empty calculation; write a name or expression inside the brackets"
                                    .to_string(),
                            )
                        } else if syntax::valid_expression(inner) {
                            Err(format!(
                                "Calculated cell [{inner}] is evaluated with the table"
                            ))
                        } else {
                            Err(format!("Invalid calculation '{inner}'"))
                        }
                    } else if domains.get(column).is_some_and(Option::is_some) {
                        // A plan decides these; whatever is written is a note to self.
                        Ok(Literal::Text(decoded.clone()))
                    } else if decoded.is_empty() {
                        Err("Missing cell value".to_string())
                    } else {
                        syntax::literal(&decoded).and_then(|v| {
                            if matches!(v, Literal::Text(_))
                                && decoded.chars().next().is_some_and(|c| {
                                    c.is_ascii_digit() || matches!(c, '$' | '-' | '+')
                                })
                            {
                                Err(format!(
                                    "Invalid scalar literal '{decoded}'; quote it to store text"
                                ))
                            } else {
                                Ok(v)
                            }
                        })
                    };
                    Cell {
                        source,
                        span,
                        value,
                        expression,
                    }
                })
                .collect(),
        );
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
                Ok(value) => {
                    let kind = literal_kind(value);
                    if let Some(expected) = table.types[column] {
                        if expected != kind {
                            problem(
                                cell.span,
                                format!(
                                    "Column '{}' expects {expected}, found {kind}",
                                    table.columns[column].name
                                ),
                            );
                        }
                    } else {
                        table.types[column] = Some(kind);
                    }
                }
            }
        }
    }
    table
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
    doc.sum_regions().iter().find_map(|region| {
        let offset = region.offset_of(doc, span)?;
        syntax::sum_scope_at(region.source(doc), offset)
    })
}
/// Align parsed tables; document feature formatting runs through services::modules.
pub fn formatting(doc: &Document) -> Vec<TextEdit> {
    grids(doc)
        .iter()
        // Never invent missing cells or repair a malformed table during formatting.
        .filter(|table| table.problems.is_empty())
        .flat_map(|table| aligned(doc, table))
        .map(|(line, text)| line_edit(doc, line, text))
        .collect()
}
/// Data tables plus plan constraint tables, which share the same grid shape.
pub fn grids(doc: &Document) -> Vec<Table> {
    doc.tables
        .iter()
        .cloned()
        .chain(doc.plans.iter().map(crate::plans_impl::grid))
        .collect()
}
/// Replacement text for every table line whose padding is off. Rows with the
/// wrong cell count are aligned as far as they go, so this also works while
/// a row is still being typed.
pub fn aligned(doc: &Document, table: &Table) -> Vec<(usize, String)> {
    let mut edits = Vec::new();
    {
        if table.separators.len() != table.columns.len() || table.columns.is_empty() {
            return edits;
        }
        let mut widths: Vec<_> = table
            .columns
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let sigil = usize::from(table.domains.get(i).is_some_and(Option::is_some));
                (c.name.width() + sigil).max(
                    3 + usize::from(table.separators[i].starts_with(':'))
                        + usize::from(table.separators[i].ends_with(':')),
                )
            })
            .collect();
        for row in &table.rows {
            for (i, c) in row.iter().enumerate().take(widths.len()) {
                widths[i] = widths[i].max(c.source.width());
            }
        }
        for line in table.header..table.end_line {
            let Some(parts) = cells(doc.line(line), line) else {
                continue;
            };
            let prefix =
                &doc.line(line)[..doc.line(line).len() - doc.line(line).trim_start().len()];
            let mut formatted = format!("{prefix}|");
            for (i, (source, _)) in parts.iter().enumerate() {
                let width = widths.get(i).copied().unwrap_or(source.width());
                let value = if line == table.header + 1 {
                    let left = source.starts_with(':');
                    let right = source.ends_with(':');
                    format!(
                        "{}{}{}",
                        if left { ":" } else { "" },
                        "-".repeat(
                            width
                                .saturating_sub(usize::from(left) + usize::from(right))
                                .max(3)
                        ),
                        if right { ":" } else { "" }
                    )
                } else {
                    source.clone()
                };
                formatted.push_str(&format!(
                    " {value}{} |",
                    " ".repeat(width.saturating_sub(value.width()))
                ));
            }
            if formatted != doc.line(line) {
                edits.push((line, formatted));
            }
        }
    }
    edits
}
pub fn line_edit(doc: &Document, line: usize, text: String) -> TextEdit {
    TextEdit::new(
        Range::new(lsp_types::Position::new(line as u32, 0), doc.line_end(line)),
        text,
    )
}
