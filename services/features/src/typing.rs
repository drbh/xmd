//! On-type formatting: realign a table when a pipe is typed and continue a
//! checklist when Enter is pressed. Shared by the native server and browser.
//! How a table is laid out is the feature modules' formatting (the bundled
//! `tables` module's); typing only decides which of its edits to keep.
use lang::common::Span;
use lang::document::{Document, byte_at, utf16};
use lang::eval::tables;
use lsp_types::{Position, Range, TextEdit};
use std::path::Path;

/// Characters that trigger `textDocument/onTypeFormatting`.
pub const TRIGGERS: [&str; 2] = ["\n", "|"];

/// `position` is the cursor after `ch` was inserted, as the protocol defines it.
pub(crate) fn on_type(
    request: &crate::Request<'_>,
    path: &Path,
    position: Position,
    ch: &str,
) -> Vec<TextEdit> {
    let Some(doc) = request.workspace().documents().get(path) else {
        return vec![];
    };
    match ch {
        "|" => pipe(request, path, doc, position),
        "\n" => newline(doc, position),
        _ => vec![],
    }
}

/// Formatting's edits to the table being typed, with the row under the
/// caret left alone until it is complete.
fn pipe(
    request: &crate::Request<'_>,
    path: &Path,
    doc: &Document,
    position: Position,
) -> Vec<TextEdit> {
    let row = position.line as usize;
    let grids = tables::grids(doc);
    let Some(table) = grids.iter().find(|t| t.header <= row && row < t.end_line) else {
        return vec![];
    };
    let line = doc.line(row);
    let Some(byte) = byte_at(line, position.character) else {
        return vec![];
    };
    // The row being typed is only touched once its closing pipe is in place and
    // every column has a cell, so the caret never lands inside fresh padding
    // while more cells are still coming.
    let complete = line[byte..].trim().is_empty()
        && tables::cells(line, row).is_some_and(|parts| parts.len() == table.columns.len());
    let Ok(edits) = crate::providers::edits(request, path, Some(position)) else {
        return vec![];
    };
    edits
        .into_iter()
        .filter(|e| {
            let line = e.range.start.line as usize;
            line == e.range.end.line as usize
                && (table.header..table.end_line).contains(&line)
                && (line != row || complete)
        })
        .flat_map(|e| {
            let whole = e.range.start.character == 0 && e.range.end == doc.line_end(row);
            if e.range.start.line as usize == row && whole {
                padding_edits(doc, row, &e.new_text)
            } else {
                vec![e]
            }
        })
        .collect()
}

/// Edits confined to whitespace runs, so a caret after the change keeps its
/// place relative to the surrounding cell text. Falls back to a whole-line edit
/// when anything but whitespace differs.
fn padding_edits(doc: &Document, row: usize, formatted: &str) -> Vec<TextEdit> {
    let line = doc.line(row);
    let old = runs(line);
    let new = runs(formatted);
    let same_shape = old.len() == new.len()
        && old
            .iter()
            .zip(&new)
            .all(|(a, b)| a.2 == b.2 && (a.2 || line[a.0..a.1] == formatted[b.0..b.1]));
    if !same_shape {
        return vec![TextEdit::new(
            Range::new(Position::new(row as u32, 0), doc.line_end(row)),
            formatted.into(),
        )];
    }
    old.iter()
        .zip(&new)
        .filter(|(a, b)| a.2 && line[a.0..a.1] != formatted[b.0..b.1])
        .map(|(a, b)| {
            TextEdit::new(
                Span::new(row, a.0, a.1).range(doc),
                formatted[b.0..b.1].into(),
            )
        })
        .collect()
}
/// Alternating whitespace and non-whitespace runs as (start, end, is_whitespace).
fn runs(s: &str) -> Vec<(usize, usize, bool)> {
    let mut result: Vec<(usize, usize, bool)> = Vec::new();
    for (i, c) in s.char_indices() {
        let ws = c.is_whitespace();
        match result.last_mut() {
            Some(last) if last.2 == ws => last.1 = i + c.len_utf8(),
            _ => result.push((i, i + c.len_utf8(), ws)),
        }
    }
    result
}

fn newline(doc: &Document, position: Position) -> Vec<TextEdit> {
    let row = position.line as usize;
    let Some(previous) = row.checked_sub(1) else {
        return vec![];
    };
    let before = doc.line(previous);
    let indent = &before[..before.len() - before.trim_start().len()];
    let body = &before[indent.len()..];
    let Some(marker) = ["-", "*", "+"]
        .into_iter()
        .flat_map(|bullet| [" ", "x", "X"].map(|state| format!("{bullet} [{state}] ")))
        .find(|m| body.starts_with(m.as_str()) || body == m.trim_end())
    else {
        return vec![];
    };
    let current = doc.line(row);
    let Some(byte) = byte_at(current, position.character) else {
        return vec![];
    };
    if !current[..byte].trim().is_empty() {
        return vec![];
    }
    if body.trim().len() == marker.trim_end().len() {
        // Enter on an empty checkbox ends the list instead of adding another.
        return vec![TextEdit::new(
            Range::new(
                Position::new(previous as u32, utf16(before, indent.len())),
                Position::new(row as u32, position.character),
            ),
            String::new(),
        )];
    }
    let fresh = format!("{}{} ", indent, &marker[..1]);
    vec![TextEdit::new(
        Range::new(
            Position::new(row as u32, 0),
            Position::new(row as u32, position.character),
        ),
        format!("{fresh}[ ] "),
    )]
}
