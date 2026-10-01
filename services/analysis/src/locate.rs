//! What sits at a position in a note, resolved once so every feature agrees on
//! it: hovers, definition and rename ask here rather than each walking the
//! note for links, cells, calculations and names in an order of its own.
use lang::common::Span;
use lang::document::{Document, byte_at};
use lang::eval::{Symbol, SymbolKind, Workspace};
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
    /// A reference to a function the prelude gives every note.
    Prelude(String, Span),
    /// A bracketed calculation in prose, by index into the note's calculations.
    Calculation(usize),
    /// Nothing more specific than the row itself.
    Row,
}

/// Whether `byte` on `row` is on `span`, counting the position just past its
/// end: a cursor right after a name is still on it.
fn touches(span: Span, row: usize, byte: usize) -> bool {
    span.line == row && span.start <= byte && byte <= span.end
}

/// Whether `byte` on `row` is on one of `span`'s characters.
fn covers(span: Span, row: usize, byte: usize) -> bool {
    span.line == row && span.start <= byte && byte < span.end
}

pub(crate) fn target(ws: &Workspace, path: &Path, position: Position) -> Option<Target> {
    let doc = ws.documents().get(path)?;
    let row = position.line as usize;
    let byte = byte_at(doc.line(row), position.character)?;
    if let Some(link) = doc.links().iter().position(|l| covers(l.span, row, byte)) {
        return Some(Target::Link(link));
    }
    for (table, t) in doc.tables().iter().enumerate() {
        for (row_index, cells) in t.rows.iter().enumerate() {
            for (column, cell) in cells.iter().enumerate().take(t.columns.len()) {
                if touches(cell.span, row, byte) {
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
    if let Some(reference) = doc
        .references()
        .iter()
        .find(|r| touches(r.span, row, byte) && ws.prelude_name(path, &r.name))
    {
        return Some(Target::Prelude(reference.name.clone(), reference.span));
    }
    // A bracketed calculation also answers on its opening bracket.
    if let Some(calculation) = doc.calculations().iter().position(|c| {
        let opening = c.span.start.saturating_sub(usize::from(c.bracketed));
        touches(Span::new(c.span.line, opening, c.span.end), row, byte)
    }) {
        return Some(Target::Calculation(calculation));
    }
    Some(Target::Row)
}

pub fn symbol_at(workspace: &Workspace, path: &Path, position: Position) -> Option<(Symbol, Span)> {
    let doc = workspace.documents().get(path)?;
    let byte = byte_at(doc.line(position.line as usize), position.character)?;
    let inside = |span: Span| touches(span, position.line as usize, byte);
    if let Some(member) = doc.members().iter().find(|m| inside(m.span))
        && let Some(symbol) = lang::eval::member_symbol(workspace, path, &member.source)
    {
        return Some((symbol, member.span));
    }
    for (table, t) in doc.tables().iter().enumerate() {
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
    doc.references()
        .iter()
        .find(|r| inside(Span::new(r.span.line, r.span.start, r.end())))
        .and_then(|r| {
            lang::eval::tables::resolve_reference(workspace, path, r)
                .ok()
                .map(|s| (s, r.span))
        })
}

/// Every name read inside `within` in the note at `path` that resolves:
/// module members first, then references, each with the span of the whole
/// read including any property.
pub(crate) fn reads_within<'a>(
    ws: &'a Workspace,
    path: &'a Path,
    within: Span,
) -> impl Iterator<Item = (Symbol, Span)> + 'a {
    let doc = &ws.documents()[path];
    let within = within.range(doc);
    let members = doc
        .members()
        .iter()
        .filter(move |m| Span::encloses(within, doc, m.span))
        .filter_map(move |m| Some((lang::eval::member_symbol(ws, path, &m.source)?, m.span)));
    let references = doc
        .references()
        .iter()
        .map(|r| (r, Span::new(r.span.line, r.span.start, r.end())))
        .filter(move |(_, span)| Span::encloses(within, doc, *span))
        .filter_map(move |(r, span)| {
            let symbol = lang::eval::tables::resolve_reference(ws, path, r).ok()?;
            Some((symbol, span))
        });
    members.chain(references)
}

/// Comments and code spans, where an editor should stay quiet: except inside
/// an attribute or a definition's expression, which read as code.
pub fn inert(doc: &Document, position: Position) -> bool {
    let row = position.line as usize;
    let Some(byte) = byte_at(doc.line(row), position.character) else {
        return true;
    };
    doc.highlights().iter().any(|h| {
        covers(h.span, row, byte)
            && (h.kind == lang::document::HighlightKind::Comment
                || h.kind == lang::document::HighlightKind::String
                    && !doc
                        .claimed_attributes()
                        .any(|(_, a)| touches(a.span, row, byte))
                    && !doc.definitions().iter().any(|d| {
                        d.expression && d.value_span.contains(doc, Span::new(row, byte, byte))
                    }))
    })
}
