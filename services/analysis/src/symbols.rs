//! Standard LSP document symbols, shared by the native server and browser adapter.
use crate::describe_impl as describe;
use lang::common::Span;
use lang::document::{Document, Named};
use lang::eval::engine::Engine;
use lang::eval::{Symbol, SymbolKind};
use lsp_types::{DocumentSymbol, Location, Range, SymbolInformation};
use std::path::Path;
use url::Url;

#[allow(deprecated)]
fn symbol(
    name: String,
    detail: String,
    kind: lsp_types::SymbolKind,
    range: Range,
    selection_range: Range,
) -> DocumentSymbol {
    DocumentSymbol {
        name,
        detail: Some(detail),
        kind,
        range,
        selection_range,
        tags: None,
        deprecated: None,
        children: None,
    }
}

/// The entry of a name a form leaves unknown or a column a table declares,
/// described as the language describes `of`.
fn described(
    engine: &mut Engine<'_>,
    doc: &Document,
    named: &Named,
    of: &Symbol,
) -> DocumentSymbol {
    let detail = describe::detail(engine, doc, of);
    let range = named.span.range(doc);
    symbol(
        named.name.clone(),
        detail,
        describe::kind(doc, of),
        range,
        range,
    )
}

/// The last row in `start..=end` with text on it, so a range never ends on
/// the blank lines before whatever follows it.
fn last_filled(doc: &Document, start: usize, mut end: usize) -> usize {
    while end > start && doc.line(end).trim().is_empty() {
        end -= 1;
    }
    end
}

/// The range of a line's text, without its indentation or trailing space.
pub fn line_range(doc: &Document, row: usize) -> Range {
    let line = doc.line(row);
    let start = line.len() - line.trim_start().len();
    Span::new(row, start, line.trim_end().len().max(start)).range(doc)
}

/// An outline entry from outside the language, such as a feature module's
/// `symbols`: it spans from `line` to its last filled line before `end_line`.
/// One on a heading's line gives that heading's entry its detail and span.
#[derive(Clone, Debug)]
pub struct Outlined {
    pub name: String,
    pub detail: String,
    pub kind: lsp_types::SymbolKind,
    pub line: usize,
    pub end_line: usize,
    pub selection: Range,
}

pub fn document_symbols(
    request: &lang::eval::RequestContext<'_>,
    path: &Path,
    outlined: Vec<Outlined>,
) -> Vec<DocumentSymbol> {
    let ws = request.workspace();

    let Some(doc) = ws.documents().get(path) else {
        return Vec::new();
    };
    let mut engine = request.engine();
    let mut entries = Vec::new();
    let heading = |line: usize| doc.sections().iter().any(|s| s.line == line);
    for (index, section) in doc.sections().iter().enumerate() {
        let selection = line_range(doc, section.line);
        // An entry outlined on the heading's line decides where it ends.
        let outline = outlined.iter().find(|o| o.line == section.line);
        let end = last_filled(
            doc,
            section.line,
            outline
                .map(|o| o.end_line)
                .unwrap_or(section.end_line)
                .saturating_sub(1),
        );
        let section_symbol = Symbol::new(path, SymbolKind::Section(index));
        entries.push(symbol(
            if section.title.is_empty() {
                "Untitled section".into()
            } else {
                section.title.clone()
            },
            match outline {
                Some(outline) => outline.detail.clone(),
                None if section.named.is_some() => {
                    describe::detail(&mut engine, doc, &section_symbol)
                }
                None => String::new(),
            },
            describe::kind(doc, &section_symbol),
            Range::new(selection.start, line_range(doc, end).end),
            selection,
        ));
    }
    for outline in outlined.into_iter().filter(|o| !heading(o.line)) {
        let end = last_filled(
            doc,
            outline.line,
            outline.end_line.saturating_sub(1).max(outline.line),
        );
        entries.push(symbol(
            outline.name,
            outline.detail,
            outline.kind,
            Range::new(
                line_range(doc, outline.line).start,
                line_range(doc, end).end,
            ),
            outline.selection,
        ));
    }
    for (i, definition) in doc.definitions().iter().enumerate() {
        let definition_symbol = Symbol::new(path, SymbolKind::Definition(i));
        let detail = describe::detail(&mut engine, doc, &definition_symbol);
        let row = definition.named.span.line;
        let first = definition.named.span.start.min(definition.value_span.start);
        let start = doc.line(row)[..first].rfind('[').unwrap_or(first);
        let table = doc.table_of(i);
        let formed = doc.form_of(i);
        let end = if doc.grid_of(i).is_some() {
            line_range(doc, doc.definition_rows(i).1).end
        } else {
            let last = definition.end;
            let end = last.end.min(doc.line(last.line).trim_end().len());
            Span::new(last.line, 0, end).range(doc).end
        };
        let full_range = Range::new(Span::new(row, start, start).range(doc).start, end);
        entries.push(symbol(
            definition.named.name.clone(),
            detail,
            describe::kind(doc, &definition_symbol),
            full_range,
            definition.named.span.range(doc),
        ));
        // A form's unknowns, and each row its table names, with the rest of
        // the row as its detail.
        if let Some(formed) = formed {
            let f = doc.forms().iter().position(|f| f.definition == i).unwrap();
            for (n, named) in ws.claimed(path, formed) {
                let variable = Symbol::new(path, SymbolKind::Variable(f, n));
                entries.push(described(&mut engine, doc, named, &variable));
            }
            for (row, cells) in formed.rows.iter().enumerate() {
                let Some((name, span)) = formed.row_name(row) else {
                    continue;
                };
                let range = span.range(doc);
                let detail: Vec<&str> = cells
                    .iter()
                    .filter(|(_, cell)| cell != span)
                    .map(|(source, _)| source.as_str())
                    .collect();
                entries.push(symbol(
                    name.clone(),
                    detail.join(" · "),
                    lsp_types::SymbolKind::FIELD,
                    range,
                    range,
                ));
            }
        }
        if let Some(table) = table {
            let t = doc.tables().iter().position(|t| t.definition == i).unwrap();
            for (c, named) in table.columns.iter().enumerate() {
                let column = Symbol::new(path, SymbolKind::Column(t, c));
                entries.push(described(&mut engine, doc, named, &column));
            }
        }
    }
    // Source order, outer before inner for nodes sharing a start. Parser-derived
    // ranges supply the hierarchy; reference occurrences never become symbols.
    entries.sort_by_key(|s| (s.range.start, std::cmp::Reverse(s.range.end)));
    let mut parents = Vec::with_capacity(entries.len());
    let mut stack: Vec<usize> = Vec::new();
    for (i, item) in entries.iter().enumerate() {
        while stack
            .last()
            .is_some_and(|p| entries[*p].range.end < item.range.end)
        {
            stack.pop();
        }
        parents.push(stack.last().copied());
        stack.push(i);
    }
    // Assemble bottom-up, avoiding recursive tree construction.
    let mut roots = Vec::new();
    while let Some(mut item) = entries.pop() {
        if let Some(children) = &mut item.children {
            children.reverse();
        }
        if let Some(parent) = parents.pop().flatten() {
            entries[parent]
                .children
                .get_or_insert_with(Vec::new)
                .push(item);
        } else {
            roots.push(item);
        }
    }
    roots.reverse();
    roots
}

/// Foldable regions: sections, tables and the tables forms take, and what a module's
/// recognizer holds open (`until`).
pub fn folding_ranges(doc: &Document) -> Vec<lsp_types::FoldingRange> {
    let mut ranges: Vec<(usize, usize, Option<lsp_types::FoldingRangeKind>)> = Vec::new();
    let mut add = |start: usize, end_exclusive: usize| {
        let end = last_filled(doc, start, end_exclusive.saturating_sub(1));
        if end > start {
            ranges.push((start, end, Some(lsp_types::FoldingRangeKind::Region)));
        }
    };
    for i in 0..doc.definitions().len() {
        let (first, last) = doc.definition_rows(i);
        add(first, last + 1);
    }
    for section in doc.sections() {
        add(section.line, section.end_line);
    }
    for found in doc.recognized().iter().filter(|f| f.rule.until.is_some()) {
        add(found.span.line, found.end);
    }
    let mut comment: Option<usize> = None;
    for (row, line) in doc.text().lines().enumerate() {
        let trimmed = line.trim();
        if comment.is_none() && trimmed.starts_with("<!--") && !trimmed.contains("-->") {
            comment = Some(row);
        } else if let Some(start) = comment
            && trimmed.contains("-->")
        {
            ranges.push((start, row, Some(lsp_types::FoldingRangeKind::Comment)));
            comment = None;
        }
    }
    ranges.sort_by_key(|(start, end, _)| (*start, *end));
    ranges.dedup_by_key(|(start, end, _)| (*start, *end));
    ranges
        .into_iter()
        .map(|(start, end, kind)| lsp_types::FoldingRange {
            start_line: start as u32,
            start_character: None,
            end_line: end as u32,
            end_character: None,
            kind,
            collapsed_text: None,
        })
        .collect()
}

/// Older clients that do not advertise hierarchicalDocumentSymbolSupport get
/// the same symbols as a flat response with explicit container names.
#[allow(deprecated)]
pub fn flat_symbols(symbols: Vec<DocumentSymbol>, uri: &Url) -> Vec<SymbolInformation> {
    let mut pending: Vec<_> = symbols.into_iter().rev().map(|s| (s, None)).collect();
    let mut result = Vec::new();
    while let Some((symbol, container_name)) = pending.pop() {
        if let Some(children) = symbol.children {
            pending.extend(
                children
                    .into_iter()
                    .rev()
                    .map(|s| (s, Some(symbol.name.clone()))),
            );
        }
        result.push(SymbolInformation {
            name: symbol.name,
            kind: symbol.kind,
            tags: symbol.tags,
            deprecated: None,
            location: Location {
                uri: lang::common::uri_from_url(uri),
                range: symbol.selection_range,
            },
            container_name,
        });
    }
    result
}
