//! Explicit computational tables. Parsing, typing, navigation and formatting are
//! shared language features, not browser-side Markdown interpretation.
use crate::{
    document::{Document, Named, Problem, Reference, Span, identifier},
    engine::{self, Value},
    workspace::{Symbol, SymbolKind, Workspace},
};
use lsp_types::{Range, TextEdit};
use std::path::Path;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Debug)]
pub struct Cell {
    pub source: String,
    pub span: Span,
    pub value: Result<Value, String>,
}
#[derive(Clone, Debug)]
pub struct Table {
    pub definition: usize,
    pub header: usize,
    pub end_line: usize,
    pub columns: Vec<Named>,
    pub rows: Vec<Vec<Cell>>,
    pub types: Vec<Option<&'static str>>,
    pub separators: Vec<String>,
    pub problems: Vec<Problem>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct TableValue {
    pub origin: Symbol,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
}

/// Pipes in quoted strings and escaped pipes are cell contents, not separators.
pub(crate) fn cells(line: &str, row: usize) -> Option<Vec<(String, Span)>> {
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

pub fn parse(doc: &Document, definition: usize, lines: &[&str]) -> Table {
    let def = &doc.definitions[definition];
    let header = def.named.span.line + 1;
    let mut table = Table {
        definition,
        header,
        end_line: header,
        columns: vec![],
        rows: vec![],
        types: vec![],
        separators: vec![],
        problems: vec![],
    };
    let mut problem = |span, message| table.problems.push(Problem { span, message });
    let Some(headers) = lines.get(header).and_then(|l| cells(l, header)) else {
        problem(
            def.value_span,
            "A table needs a pipe-delimited header on the next line".into(),
        );
        return table;
    };
    for (name, span) in headers {
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
            problem(
                Span::new(row, 0, line.len()),
                format!(
                    "Expected {} cells, found {}",
                    table.columns.len(),
                    parts.len()
                ),
            );
        }
        table.rows.push(
            parts
                .into_iter()
                .map(|(source, span)| {
                    let decoded = source.replace("\\|", "|");
                    let value = if decoded.is_empty() {
                        Err("Missing cell value".into())
                    } else {
                        engine::literal(&decoded).and_then(|v| {
                            if matches!(v, Value::Text(_))
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
                    }
                })
                .collect(),
        );
    }
    table.types = vec![None; table.columns.len()];
    for row in &table.rows {
        for (column, cell) in row.iter().enumerate().take(table.columns.len()) {
            match &cell.value {
                Err(message) => problem(cell.span, message.clone()),
                Ok(value) => {
                    let kind = value.type_name();
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

pub fn origin(ws: &Workspace, path: &Path, name: &str) -> Result<Symbol, String> {
    let mut symbol = ws.resolve(path, name)?;
    for _ in 0..64 {
        let doc = &ws.documents[&symbol.path];
        if let SymbolKind::Definition(index) = symbol.kind {
            if doc.tables.iter().any(|t| t.definition == index) {
                return Ok(symbol);
            }
            let def = &doc.definitions[index];
            if def.expression
                && let Some(alias) = engine::simple_name(&def.source)
            {
                symbol = ws.resolve(&symbol.path, &alias)?;
                continue;
            }
        }
        return Err(format!("'{name}' is not a table"));
    }
    Err("Table alias chain is cyclic or too deep".into())
}
pub fn table<'a>(ws: &'a Workspace, symbol: &Symbol) -> Option<&'a Table> {
    if let SymbolKind::Definition(index) = symbol.kind {
        ws.documents[&symbol.path]
            .tables
            .iter()
            .find(|t| t.definition == index)
    } else {
        None
    }
}

/// Find the innermost sum row scope, including incomplete formulas while typing.
pub fn scope_at(doc: &Document, span: Span) -> Option<String> {
    crate::refactor::expression_regions(doc)
        .into_iter()
        .find_map(|region| {
            if region.line != span.line || span.start < region.start || span.start > region.end {
                return None;
            }
            engine::sum_scope_at(
                &doc.line(region.line)[region.start..region.end],
                span.start - region.start,
            )
        })
}
pub fn resolve_reference(
    ws: &Workspace,
    path: &Path,
    reference: &Reference,
) -> Result<Symbol, String> {
    let Some(name) = scope_at(&ws.documents[path], reference.span) else {
        return ws.resolve(path, &reference.name);
    };
    let target = origin(ws, path, &name)?;
    let doc = &ws.documents[&target.path];
    let (index, table) = doc
        .tables
        .iter()
        .enumerate()
        .find(|(_, t)| matches!(target.kind, SymbolKind::Definition(i) if i == t.definition))
        .unwrap();
    let matches: Vec<_> = table
        .columns
        .iter()
        .enumerate()
        .filter(|(_, c)| c.name == reference.name)
        .collect();
    match matches.as_slice() {
        [(column, _)] => Ok(Symbol {
            path: target.path,
            kind: SymbolKind::Column(index, *column),
        }),
        [] => Err(format!(
            "Unknown column '{}' in table '{name}'",
            reference.name
        )),
        _ => Err(format!(
            "Ambiguous column '{}' in table '{name}'",
            reference.name
        )),
    }
}
pub fn validate_rename(ws: &Workspace, symbol: &Symbol, name: &str) -> Result<(), String> {
    if !identifier(name) || matches!(name, "true" | "false") {
        return Err("Use an identifier other than true or false".into());
    }
    let conflict = if let SymbolKind::Column(t, column) = symbol.kind {
        ws.documents[&symbol.path].tables[t]
            .columns
            .iter()
            .enumerate()
            .any(|(i, c)| i != column && c.name == name)
    } else {
        ws.symbols()
            .iter()
            .any(|s| s != symbol && ws.named(s).name == name)
    };
    if conflict {
        Err("That name already exists in this scope".into())
    } else {
        Ok(())
    }
}

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
        .chain(doc.plans.iter().map(crate::plans::grid))
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
                c.name.width().max(
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
