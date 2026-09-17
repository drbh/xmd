//! Standard LSP document symbols, shared by the native server and browser adapter.
use crate::{
    document::{Document, Span},
    engine::Engine,
    workspace::{Symbol, SymbolKind, Workspace},
};
use chrono::{DateTime, FixedOffset};
use lsp_types::{DocumentSymbol, Location, Range, SymbolInformation, Url};
use std::path::Path;

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

fn line_range(doc: &Document, row: usize) -> Range {
    let line = doc.line(row);
    Span::new(
        row,
        line.len() - line.trim_start().len(),
        line.trim_end()
            .len()
            .max(line.len() - line.trim_start().len()),
    )
    .range(&doc.text)
}

pub fn document_symbols(
    ws: &Workspace,
    path: &Path,
    now: DateTime<FixedOffset>,
) -> Vec<DocumentSymbol> {
    let Some(doc) = ws.documents.get(path) else {
        return Vec::new();
    };
    let mut engine = Engine::at(ws, now);
    let mut entries = Vec::new();
    for section in &doc.sections {
        let selection = line_range(doc, section.line);
        let mut end = section.end_line.saturating_sub(1);
        while end > section.line && doc.line(end).trim().is_empty() {
            end -= 1;
        }
        entries.push(symbol(
            if section.title.is_empty() {
                "Untitled section".into()
            } else {
                section.title.clone()
            },
            section
                .named
                .as_ref()
                .map(|n| format!(":{}", n.name))
                .unwrap_or_default(),
            lsp_types::SymbolKind::NAMESPACE,
            Range::new(selection.start, line_range(doc, end).end),
            selection,
        ));
    }
    // Extend each task's range through all descendants, without consuming a sibling.
    let mut task_ends: Vec<_> = doc.tasks.iter().map(|t| t.line).collect();
    for (i, task) in doc.tasks.iter().enumerate().rev() {
        if let Some(parent) = task.parent {
            task_ends[parent] = task_ends[parent].max(task_ends[i]);
        }
    }
    for (i, task) in doc.tasks.iter().enumerate() {
        let selection = task
            .named
            .as_ref()
            .map(|n| n.span.range(&doc.text))
            .unwrap_or_else(|| line_range(doc, task.line));
        let status = if engine.task_done(path, i) {
            "Complete"
        } else {
            "Incomplete"
        };
        entries.push(symbol(
            if task.title.is_empty() {
                "Untitled task".into()
            } else {
                task.title.clone()
            },
            task.named
                .as_ref()
                .map(|n| format!("{status} · :{}", n.name))
                .unwrap_or_else(|| status.into()),
            lsp_types::SymbolKind::BOOLEAN,
            Range::new(
                line_range(doc, task.line).start,
                line_range(doc, task_ends[i]).end,
            ),
            selection,
        ));
    }
    for event in &doc.events {
        let range = line_range(doc, event.line);
        entries.push(symbol(
            if event.title.is_empty() {
                "Untitled event".into()
            } else {
                event.title.clone()
            },
            event
                .attributes
                .get("at")
                .map(|a| format!("@at({})", a.value))
                .unwrap_or_default(),
            lsp_types::SymbolKind::EVENT,
            range,
            range,
        ));
    }
    for (i, definition) in doc.definitions.iter().enumerate() {
        let detail = match engine.symbol(&Symbol {
            path: path.into(),
            kind: SymbolKind::Definition(i),
        }) {
            Ok(value) => format!("{} · {}", value.type_name(), value.display()),
            Err(error) => format!("Error · {error}"),
        };
        let row = definition.named.span.line;
        let first = definition.named.span.start.min(definition.value_span.start);
        let start = doc.line(row)[..first].rfind('[').unwrap_or(first);
        let table = doc.tables.iter().find(|t| t.definition == i);
        let full_range = if let Some(table) = table {
            Range::new(
                Span::new(row, start, start).range(&doc.text).start,
                line_range(doc, table.end_line.saturating_sub(1)).end,
            )
        } else {
            Span::new(
                row,
                start,
                definition.end.end.min(doc.line(row).trim_end().len()),
            )
            .range(&doc.text)
        };
        entries.push(symbol(
            definition.named.name.clone(),
            detail,
            if table.is_some() {
                lsp_types::SymbolKind::STRUCT
            } else if definition.expression {
                lsp_types::SymbolKind::VARIABLE
            } else {
                lsp_types::SymbolKind::CONSTANT
            },
            full_range,
            definition.named.span.range(&doc.text),
        ));
        if let Some(table) = table {
            for (column, named) in table.columns.iter().enumerate() {
                let range = named.span.range(&doc.text);
                entries.push(symbol(
                    named.name.clone(),
                    table.types[column].unwrap_or("Unknown").into(),
                    lsp_types::SymbolKind::FIELD,
                    range,
                    range,
                ));
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
                uri: uri.clone(),
                range: symbol.selection_range,
            },
            container_name,
        });
    }
    result
}
