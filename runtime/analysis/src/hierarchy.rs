//! Dependency graph over LSP call hierarchy. "Incoming calls" are the values,
//! tasks and checklists that depend on an item; "outgoing calls" are what the
//! item itself depends on. Shared by the native server and deterministic tests.
use crate::describe_impl as describe;
use crate::locate::{reads_within, symbol_at};
use lang::common::Span;
use lang::eval::{Symbol, SymbolKind, Workspace};
use lang::model::Document;
use lsp_types::{CallHierarchyItem, Position, Range};
use std::path::{Path, PathBuf};

/// The graph node under the cursor: a named value, column, task line, or a
/// reference to one of those.
pub fn prepare(ws: &Workspace, path: &Path, position: Position) -> Option<Symbol> {
    if let Some((symbol, _)) = symbol_at(ws, path, position) {
        return Some(symbol);
    }
    let doc = ws.documents().get(path)?;
    let row = position.line as usize;
    doc.tasks
        .iter()
        .position(|t| t.line == row)
        .map(|i| Symbol::new(path, SymbolKind::Task(i)))
        .or_else(|| {
            doc.sections
                .iter()
                .position(|s| s.line == row && s.named.is_some())
                .map(|i| Symbol::new(path, SymbolKind::Section(i)))
        })
}

/// Display name for any node, including tasks without a `:name`.
pub fn label(ws: &Workspace, symbol: &Symbol) -> String {
    let doc = &ws.documents()[&symbol.path];
    match symbol.kind {
        SymbolKind::Task(i) => doc.tasks[i]
            .named
            .as_ref()
            .map(|n| n.name.clone())
            .unwrap_or_else(|| doc.tasks[i].title.clone()),
        SymbolKind::Section(i) => doc.sections[i]
            .named
            .as_ref()
            .map(|n| n.name.clone())
            .unwrap_or_else(|| doc.sections[i].title.clone()),
        _ => ws.named(symbol).name.clone(),
    }
}
pub fn selection(doc: &Document, symbol: &Symbol) -> Span {
    match symbol.kind {
        SymbolKind::Task(i) => {
            let task = &doc.tasks[i];
            task.named.as_ref().map(|n| n.span).unwrap_or_else(|| {
                let start = task.checkbox.end + 1;
                Span::new(
                    task.line,
                    start.min(doc.line(task.line).len()),
                    start + task.title.len(),
                )
            })
        }
        SymbolKind::Section(i) => {
            let section = &doc.sections[i];
            section
                .named
                .as_ref()
                .map(|n| n.span)
                .unwrap_or_else(|| Span::new(section.line, 0, doc.line(section.line).len()))
        }
        SymbolKind::Definition(i) => doc.definitions[i].named.span,
        SymbolKind::Column(t, c) => doc.tables[t].columns[c].span,
        SymbolKind::Variable(p, n) => doc.plans[p].names[n].span,
    }
}
fn extent(doc: &Document, symbol: &Symbol) -> Range {
    let line = |row: usize| Range::new(Position::new(row as u32, 0), doc.line_end(row));
    match symbol.kind {
        SymbolKind::Task(i) => line(doc.tasks[i].line),
        SymbolKind::Section(i) => line(doc.sections[i].line),
        SymbolKind::Definition(i) => {
            let (first, last) = doc.definition_rows(i);
            Range::new(Position::new(first as u32, 0), doc.line_end(last))
        }
        SymbolKind::Column(t, _) => line(doc.tables[t].header),
        SymbolKind::Variable(p, n) => line(doc.plans[p].names[n].span.line),
    }
}

pub fn item(request: &lang::eval::RequestContext<'_>, symbol: &Symbol) -> CallHierarchyItem {
    let ws = request.workspace();
    let doc = &ws.documents()[&symbol.path];
    let mut engine = request.engine();
    let kind = describe::kind(doc, symbol);
    let detail = describe::detail(&mut engine, doc, symbol);
    CallHierarchyItem {
        name: label(ws, symbol),
        kind,
        tags: None,
        detail: Some(detail),
        uri: lang::common::uri_from_url(&lang::common::uri(&symbol.path)),
        range: extent(doc, symbol),
        selection_range: selection(doc, symbol).range(&doc.text),
        data: Some(encode(symbol)),
    }
}
pub fn encode(symbol: &Symbol) -> serde_json::Value {
    let (kind, a, b) = match symbol.kind {
        SymbolKind::Definition(i) => ("definition", i, 0),
        SymbolKind::Task(i) => ("task", i, 0),
        SymbolKind::Section(i) => ("section", i, 0),
        SymbolKind::Column(t, c) => ("column", t, c),
        SymbolKind::Variable(p, n) => ("variable", p, n),
    };
    serde_json::json!({"path": symbol.path, "kind": kind, "index": a, "column": b})
}
/// Items round-trip through the client; reject anything stale or foreign.
pub fn decode(ws: &Workspace, item: &CallHierarchyItem) -> Option<Symbol> {
    let data = item.data.as_ref()?;
    let path: PathBuf = serde_json::from_value(data.get("path")?.clone()).ok()?;
    let doc = ws.documents().get(&path)?;
    let index = data.get("index")?.as_u64()? as usize;
    let column = data.get("column")?.as_u64()? as usize;
    let kind = match data.get("kind")?.as_str()? {
        "definition" if index < doc.definitions.len() => SymbolKind::Definition(index),
        "task" if index < doc.tasks.len() => SymbolKind::Task(index),
        "section" if index < doc.sections.len() => SymbolKind::Section(index),
        "variable" if doc.plans.get(index).is_some_and(|p| column < p.names.len()) => {
            SymbolKind::Variable(index, column)
        }
        "column"
            if doc
                .tables
                .get(index)
                .is_some_and(|t| column < t.columns.len()) =>
        {
            SymbolKind::Column(index, column)
        }
        _ => return None,
    };
    Some(Symbol { path, kind })
}

/// Everything `symbol` reads, with the spans in its own note where each read happens.
pub fn dependencies(ws: &Workspace, symbol: &Symbol) -> Vec<(Symbol, Vec<Span>)> {
    let doc = &ws.documents()[&symbol.path];
    let mut edges: Vec<(Symbol, Vec<Span>)> = Vec::new();
    let own_variable = |target: &Symbol| matches!((&symbol.kind, &target.kind), (SymbolKind::Definition(i), SymbolKind::Variable(p, _)) if target.path == symbol.path && doc.plans[*p].definition == *i);
    let mut add = |target: Symbol, span: Span| {
        if own_variable(&target) {
            return;
        }
        match edges.iter_mut().find(|(t, _)| *t == target) {
            Some((_, spans)) => spans.push(span),
            None => edges.push((target, vec![span])),
        }
    };
    let mut references = |within: Span| {
        for (target, span) in reads_within(ws, &symbol.path, within) {
            add(target, span);
        }
    };
    match symbol.kind {
        SymbolKind::Definition(i) => {
            let def = &doc.definitions[i];
            if let Some(plan) = doc.plan_of(i) {
                for region in lang::eval::plans::regions(plan) {
                    references(region);
                }
            } else if let Some(table) = doc.table_of(i) {
                for cell in table.rows.iter().flatten() {
                    if let Some((_, span)) = &cell.expression {
                        references(*span);
                    }
                }
            } else if def.expression {
                references(def.value_span);
            }
        }
        SymbolKind::Variable(p, n) => add(
            symbol.sibling(SymbolKind::Definition(doc.plans[p].definition)),
            doc.plans[p].names[n].span,
        ),
        SymbolKind::Task(i) => {
            for attr in doc.tasks[i].attributes.values() {
                references(attr.value_span);
            }
            for (j, child) in doc.tasks.iter().enumerate() {
                if child.parent == Some(i) {
                    add(symbol.sibling(SymbolKind::Task(j)), child.checkbox);
                }
            }
        }
        SymbolKind::Section(i) => {
            for j in doc.section_tasks(i) {
                add(symbol.sibling(SymbolKind::Task(j)), doc.tasks[j].checkbox);
            }
        }
        SymbolKind::Column(t, c) => add(
            symbol.sibling(SymbolKind::Definition(doc.tables[t].definition)),
            doc.tables[t].columns[c].span,
        ),
    }
    edges
}
/// Everything that reads `symbol`, with the spans in each reader's note.
pub fn dependents(ws: &Workspace, symbol: &Symbol) -> Vec<(Symbol, Vec<Span>)> {
    nodes(ws)
        .into_iter()
        .filter_map(|node| {
            dependencies(ws, &node)
                .into_iter()
                .find(|(target, _)| target == symbol)
                .map(|(_, spans)| (node, spans))
        })
        .collect()
}
/// Every node that can hold an edge, including unnamed tasks and sections.
pub fn nodes(ws: &Workspace) -> Vec<Symbol> {
    ws.documents()
        .iter()
        .flat_map(|(path, doc)| {
            let mut kinds: Vec<SymbolKind> = (0..doc.definitions.len())
                .map(SymbolKind::Definition)
                .chain((0..doc.tasks.len()).map(SymbolKind::Task))
                .chain((0..doc.sections.len()).map(SymbolKind::Section))
                .chain(doc.tables.iter().enumerate().flat_map(|(t, table)| {
                    (0..table.columns.len()).map(move |c| SymbolKind::Column(t, c))
                }))
                .collect();
            for (p, plan) in doc.plans.iter().enumerate() {
                kinds.extend(
                    ws.plan_variables(path, plan)
                        .into_iter()
                        .map(|(n, _)| SymbolKind::Variable(p, n)),
                );
            }
            kinds
                .into_iter()
                .map(|kind| Symbol::new(path.clone(), kind))
                .collect::<Vec<_>>()
        })
        .collect()
}
pub fn ranges(ws: &Workspace, path: &Path, spans: &[Span]) -> Vec<Range> {
    let text = &ws.documents()[path].text;
    spans.iter().map(|s| s.range(text)).collect()
}
