//! Standard LSP document symbols, shared by the native server and browser adapter.
use crate::describe_impl as describe;
use lang::common::Span;
use lang::eval::{Symbol, SymbolKind};
use lang::model::Document;
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

/// The last row in `start..=end` with text on it, so a range never ends on
/// the blank lines before whatever follows it.
fn last_filled(doc: &Document, start: usize, mut end: usize) -> usize {
    while end > start && doc.line(end).trim().is_empty() {
        end -= 1;
    }
    end
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
    request: &lang::eval::RequestContext<'_>,
    path: &Path,
) -> Vec<DocumentSymbol> {
    let ws = request.workspace();
    let now = request.now();

    let Some(doc) = ws.documents().get(path) else {
        return Vec::new();
    };
    let mut engine = request.engine();
    let mut entries = Vec::new();
    let dates = lang::eval::itinerary::dates(ws.modules(), &doc.days, now.date_naive());
    let day_detail = |day: &lang::eval::itinerary::Day, date: &Option<chrono::NaiveDate>| {
        format!(
            "{} stops{}",
            day.stops.len(),
            date.map(|d| format!(" · {}", d.format("%A %Y-%m-%d")))
                .unwrap_or_default()
        )
    };
    for (index, section) in doc.sections.iter().enumerate() {
        let selection = line_range(doc, section.line);
        // A day heading ends where the next day starts, even without a heading.
        let day = doc
            .days
            .iter()
            .zip(&dates)
            .find(|(d, _)| d.line == section.line);
        let end = last_filled(
            doc,
            section.line,
            day.map(|(d, _)| d.end_line)
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
            match day {
                Some((day, date)) => day_detail(day, date),
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
        let task_symbol = Symbol::new(path, SymbolKind::Task(i));
        entries.push(symbol(
            if task.title.is_empty() {
                "Untitled task".into()
            } else {
                task.title.clone()
            },
            describe::detail(&mut engine, doc, &task_symbol),
            describe::kind(doc, &task_symbol),
            Range::new(
                line_range(doc, task.line).start,
                line_range(doc, task_ends[i]).end,
            ),
            selection,
        ));
    }
    for (day, date) in doc.days.iter().zip(&dates) {
        let selection = day.date_span.range(&doc.text);
        let end = last_filled(doc, day.line, day.end_line.saturating_sub(1).max(day.line));
        // A day written as a heading is already a section symbol.
        if !doc.sections.iter().any(|s| s.line == day.line) {
            entries.push(symbol(
                format!(
                    "{} {}{}",
                    lang::eval::itinerary::month_name(day.month),
                    day.day,
                    day.places
                        .as_ref()
                        .map(|(p, _)| format!(" · {p}"))
                        .unwrap_or_default()
                ),
                day_detail(day, date),
                lsp_types::SymbolKind::NAMESPACE,
                Range::new(selection.start, line_range(doc, end).end),
                selection,
            ));
        }
        for stop in &day.stops {
            let selection = stop.title_span.range(&doc.text);
            entries.push(symbol(
                stop.title.clone(),
                match stop.kind {
                    Some(kind) => {
                        format!(
                            "{} · {}",
                            lang::eval::itinerary::display_time(ws.modules(), stop),
                            kind.name
                        )
                    }
                    None => lang::eval::itinerary::display_time(ws.modules(), stop),
                },
                lsp_types::SymbolKind::EVENT,
                Range::new(
                    line_range(doc, stop.line).start,
                    line_range(doc, stop.end_line.saturating_sub(1).max(stop.line)).end,
                ),
                selection,
            ));
        }
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
        let definition_symbol = Symbol::new(path, SymbolKind::Definition(i));
        let detail = describe::detail(&mut engine, doc, &definition_symbol);
        let row = definition.named.span.line;
        let first = definition.named.span.start.min(definition.value_span.start);
        let start = doc.line(row)[..first].rfind('[').unwrap_or(first);
        let table = doc.table_of(i);
        let plan = doc.plan_of(i);
        let full_range = if table.is_some() || plan.is_some() {
            Range::new(
                Span::new(row, start, start).range(&doc.text).start,
                line_range(doc, doc.definition_rows(i).1).end,
            )
        } else {
            Range::new(
                Span::new(row, start, start).range(&doc.text).start,
                Span::new(
                    definition.end.line,
                    0,
                    definition
                        .end
                        .end
                        .min(doc.line(definition.end.line).trim_end().len()),
                )
                .range(&doc.text)
                .end,
            )
        };
        entries.push(symbol(
            definition.named.name.clone(),
            detail,
            describe::kind(doc, &definition_symbol),
            full_range,
            definition.named.span.range(&doc.text),
        ));
        if let Some(plan) = plan {
            let p = doc.plans.iter().position(|p| p.definition == i).unwrap();
            for (n, named) in ws.plan_variables(path, plan) {
                let range = named.span.range(&doc.text);
                let variable = Symbol::new(path, SymbolKind::Variable(p, n));
                entries.push(symbol(
                    named.name.clone(),
                    describe::detail(&mut engine, doc, &variable),
                    describe::kind(doc, &variable),
                    range,
                    range,
                ));
            }
            for constraint in &plan.constraints {
                let range = constraint.named.span.range(&doc.text);
                entries.push(symbol(
                    constraint.named.name.clone(),
                    constraint.source.clone(),
                    lsp_types::SymbolKind::FIELD,
                    range,
                    range,
                ));
            }
        }
        if let Some(table) = table {
            let t = doc.tables.iter().position(|t| t.definition == i).unwrap();
            for (c, named) in table.columns.iter().enumerate() {
                let range = named.span.range(&doc.text);
                let column = Symbol::new(path, SymbolKind::Column(t, c));
                entries.push(symbol(
                    named.name.clone(),
                    describe::detail(&mut engine, doc, &column),
                    describe::kind(doc, &column),
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

/// Foldable regions: sections, itinerary days and stops, tables and plans.
pub fn folding_ranges(doc: &Document) -> Vec<lsp_types::FoldingRange> {
    let mut ranges: Vec<(usize, usize, Option<lsp_types::FoldingRangeKind>)> = Vec::new();
    let mut add = |start: usize, end_exclusive: usize| {
        let end = last_filled(doc, start, end_exclusive.saturating_sub(1));
        if end > start {
            ranges.push((start, end, Some(lsp_types::FoldingRangeKind::Region)));
        }
    };
    for i in 0..doc.definitions.len() {
        let (first, last) = doc.definition_rows(i);
        add(first, last + 1);
    }
    for section in &doc.sections {
        add(section.line, section.end_line);
    }
    for day in &doc.days {
        add(day.line, day.end_line);
        for stop in &day.stops {
            add(stop.line, stop.end_line);
        }
    }
    let mut comment: Option<usize> = None;
    for (row, line) in doc.text.lines().enumerate() {
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
