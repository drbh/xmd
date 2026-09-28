//! What sits at a position in a note, resolved once so every feature agrees on
//! it: hovers, definition and rename ask here rather than each walking the
//! note for links, cells, calculations and names in an order of its own.
use lang::common::Span;
use lang::eval::{Symbol, SymbolKind, Workspace};
use lang::model::{Document, byte_at};
use lsp_types::Position;
use std::path::Path;

/// The thing under a position, most specific first.
pub(crate) enum Target {
    /// A raw link, by index into the note's links.
    Link(usize),
    Cell {
        table: usize,
        row: usize,
        column: usize,
    },
    /// A declared name, a reference that resolves to one, or a table column.
    Symbol(Symbol, Span),
    /// A bracketed calculation in prose, by index into the note's calculations.
    Calculation(usize),
    /// Nothing more specific than the task on this row.
    Task(usize),
}

pub(crate) fn target(ws: &Workspace, path: &Path, position: Position) -> Option<Target> {
    let doc = ws.documents().get(path)?;
    let row = position.line as usize;
    let byte = byte_at(doc.line(row), position.character)?;
    if let Some(link) = doc
        .links
        .iter()
        .position(|l| l.span.line == row && l.span.start <= byte && byte < l.span.end)
    {
        return Some(Target::Link(link));
    }
    for (table, t) in doc.tables.iter().enumerate() {
        for (row_index, cells) in t.rows.iter().enumerate() {
            for (column, cell) in cells.iter().enumerate().take(t.columns.len()) {
                if cell.span.line == row && byte >= cell.span.start && byte <= cell.span.end {
                    return Some(Target::Cell {
                        table,
                        row: row_index,
                        column,
                    });
                }
            }
        }
    }
    if let Some((symbol, span)) = symbol_at(ws, path, position) {
        return Some(Target::Symbol(symbol, span));
    }
    if let Some(calculation) = doc.calculations.iter().position(|c| {
        c.span.line == row && byte + usize::from(c.bracketed) >= c.span.start && byte <= c.span.end
    }) {
        return Some(Target::Calculation(calculation));
    }
    doc.tasks
        .iter()
        .position(|t| t.line == row)
        .map(Target::Task)
}

pub fn symbol_at(workspace: &Workspace, path: &Path, position: Position) -> Option<(Symbol, Span)> {
    let doc = workspace.documents().get(path)?;
    let byte = byte_at(doc.line(position.line as usize), position.character)?;
    let inside =
        |span: Span| span.line == position.line as usize && byte >= span.start && byte <= span.end;
    if let Some(member) = doc.members.iter().find(|m| inside(m.span))
        && let Some(symbol) = lang::eval::member_symbol(workspace, path, &member.source)
    {
        return Some((symbol, member.span));
    }
    for (table, t) in doc.tables.iter().enumerate() {
        for (column, c) in t.columns.iter().enumerate() {
            if inside(c.span) {
                return Some((Symbol::new(path, SymbolKind::Column(table, column)), c.span));
            }
        }
    }
    for symbol in workspace.symbols().into_iter().filter(|s| s.path == path) {
        let span = workspace.named(&symbol).span;
        if inside(span) {
            return Some((symbol, span));
        }
    }
    doc.references
        .iter()
        .find(|r| inside(Span::new(r.span.line, r.span.start, r.end())))
        .and_then(|r| {
            lang::eval::tables::resolve_reference(workspace, path, r)
                .ok()
                .map(|s| (s, r.span))
        })
}

/// Comments and code spans, where an editor should stay quiet: except inside
/// an attribute or a definition's expression, which read as code.
pub(crate) fn inert(doc: &Document, position: Position) -> bool {
    let row = position.line as usize;
    let Some(byte) = byte_at(doc.line(row), position.character) else {
        return true;
    };
    doc.highlights.iter().any(|h| {
        h.span.line == row
            && byte >= h.span.start
            && byte < h.span.end
            && (h.kind == lang::model::HighlightKind::Comment
                || h.kind == lang::model::HighlightKind::String
                    && !doc
                        .tasks
                        .iter()
                        .flat_map(|t| t.attributes.values())
                        .chain(doc.events.iter().flat_map(|e| e.attributes.values()))
                        .any(|a| a.span.line == row && byte >= a.span.start && byte <= a.span.end)
                    && !doc.definitions.iter().any(|d| {
                        d.expression && d.value_span.contains(&doc.text, Span::new(row, byte, byte))
                    }))
    })
}
