//! Dependency graph over LSP call hierarchy. "Incoming calls" are the values,
//! tasks and checklists that depend on an item; "outgoing calls" are what the
//! item itself depends on. Shared by the native server and deterministic tests.
use crate::{
    document::{Document, Span},
    engine::{Engine, Value},
    intelligence::symbol_at,
    workspace::{Symbol, SymbolKind, Workspace},
};
use chrono::{DateTime, FixedOffset};
use lsp_types::{CallHierarchyItem, Position, Range};
use std::path::{Path, PathBuf};

/// The graph node under the cursor: a named value, column, task line, or a
/// reference to one of those.
pub fn prepare(ws: &Workspace, path: &Path, position: Position) -> Option<Symbol> {
    if let Some((symbol, _)) = symbol_at(ws, path, position) {
        return Some(symbol);
    }
    let doc = ws.documents.get(path)?;
    let row = position.line as usize;
    doc.tasks
        .iter()
        .position(|t| t.line == row)
        .map(|i| Symbol {
            path: path.into(),
            kind: SymbolKind::Task(i),
        })
        .or_else(|| {
            doc.sections
                .iter()
                .position(|s| s.line == row && s.named.is_some())
                .map(|i| Symbol {
                    path: path.into(),
                    kind: SymbolKind::Section(i),
                })
        })
}

/// Display name for any node, including tasks without a `:name`.
pub fn label(ws: &Workspace, symbol: &Symbol) -> String {
    let doc = &ws.documents[&symbol.path];
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
pub(crate) fn selection(doc: &Document, symbol: &Symbol) -> Span {
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
            let def = &doc.definitions[i];
            let end = doc
                .tables
                .iter()
                .find(|t| t.definition == i)
                .map(|t| t.end_line)
                .or_else(|| {
                    doc.plans
                        .iter()
                        .find(|p| p.definition == i)
                        .map(|p| p.end_line)
                });
            if let Some(end) = end {
                Range::new(
                    Position::new(def.named.span.line as u32, 0),
                    doc.line_end(end.saturating_sub(1).max(def.named.span.line)),
                )
            } else {
                line(def.named.span.line)
            }
        }
        SymbolKind::Column(t, _) => line(doc.tables[t].header),
        SymbolKind::Variable(p, n) => line(doc.plans[p].names[n].span.line),
    }
}

pub fn item(ws: &Workspace, symbol: &Symbol, now: DateTime<FixedOffset>) -> CallHierarchyItem {
    let doc = &ws.documents[&symbol.path];
    let mut engine = Engine::at(ws, now);
    let (kind, detail) = match symbol.kind {
        SymbolKind::Task(i) => {
            let done = engine.task_done(&symbol.path, i);
            let blocked = engine.blocked(&symbol.path, i);
            (
                lsp_types::SymbolKind::BOOLEAN,
                match blocked {
                    _ if done => "task · complete".to_string(),
                    Ok(names) if !names.is_empty() => {
                        format!("task · blocked by {}", names.join(", "))
                    }
                    Ok(_) => "task · incomplete".into(),
                    Err(e) => format!("task · {e}"),
                },
            )
        }
        SymbolKind::Section(_) => {
            let detail = match engine.symbol(symbol) {
                Ok(Value::Tasks(tasks)) => {
                    let done = tasks
                        .iter()
                        .filter(|(p, i)| engine.task_done(p, *i))
                        .count();
                    format!("checklist · {done}/{} complete", tasks.len())
                }
                Ok(v) => v.display(),
                Err(e) => e,
            };
            (lsp_types::SymbolKind::NAMESPACE, detail)
        }
        SymbolKind::Column(t, c) => (
            lsp_types::SymbolKind::FIELD,
            format!(
                "{} · column of {}",
                doc.tables[t].types[c]
                    .map(|t| t.as_str())
                    .unwrap_or("Unknown"),
                doc.definitions[doc.tables[t].definition].named.name
            ),
        ),
        SymbolKind::Variable(p, _) => (
            lsp_types::SymbolKind::VARIABLE,
            match engine.symbol(symbol) {
                Ok(v) => format!(
                    "decision variable of {} · {}",
                    doc.definitions[doc.plans[p].definition].named.name,
                    v.display()
                ),
                Err(e) => format!("decision variable · {e}"),
            },
        ),
        SymbolKind::Definition(i) => {
            let table = doc.tables.iter().any(|t| t.definition == i);
            let plan = doc.plans.iter().any(|p| p.definition == i);
            let kind = if table || plan {
                lsp_types::SymbolKind::STRUCT
            } else if doc.definitions[i].expression {
                lsp_types::SymbolKind::VARIABLE
            } else {
                lsp_types::SymbolKind::CONSTANT
            };
            let detail = match engine.symbol(symbol) {
                Ok(Value::Table(t)) => format!("table · {} rows", t.rows.len()),
                Ok(Value::Plan(p)) => format!(
                    "plan · {} {} · {} variables",
                    p.goal.keyword(),
                    p.objective.display(),
                    p.variables.len()
                ),
                Ok(v) => format!("{} · {}", v.type_name(), v.display()),
                Err(e) => e,
            };
            (kind, detail)
        }
    };
    CallHierarchyItem {
        name: label(ws, symbol),
        kind,
        tags: None,
        detail: Some(detail),
        uri: crate::paths::file_url(&symbol.path).unwrap(),
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
    let doc = ws.documents.get(&path)?;
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
    let doc = &ws.documents[&symbol.path];
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
    let mut references =
        |within: Span| {
            for member in doc
                .members
                .iter()
                .filter(|m| within.contains(&doc.text, m.span))
            {
                if let Some(target) =
                    crate::model::imports::member_symbol(ws, &symbol.path, &member.source)
                {
                    add(target, member.span);
                }
            }
            for reference in doc.references.iter().filter(|r| {
                within.contains(&doc.text, Span::new(r.span.line, r.span.start, r.end()))
            }) {
                if let Ok(target) = crate::tables::resolve_reference(ws, &symbol.path, reference) {
                    add(
                        target,
                        Span::new(reference.span.line, reference.span.start, reference.end()),
                    );
                }
            }
        };
    match symbol.kind {
        SymbolKind::Definition(i) => {
            let def = &doc.definitions[i];
            if let Some(plan) = doc.plans.iter().find(|p| p.definition == i) {
                for region in crate::plans::regions(plan) {
                    references(region);
                }
            } else if let Some(table) = doc.tables.iter().find(|t| t.definition == i) {
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
            Symbol {
                path: symbol.path.clone(),
                kind: SymbolKind::Definition(doc.plans[p].definition),
            },
            doc.plans[p].names[n].span,
        ),
        SymbolKind::Task(i) => {
            for attr in doc.tasks[i].attributes.values() {
                references(attr.value_span);
            }
            for (j, child) in doc.tasks.iter().enumerate() {
                if child.parent == Some(i) {
                    add(
                        Symbol {
                            path: symbol.path.clone(),
                            kind: SymbolKind::Task(j),
                        },
                        child.checkbox,
                    );
                }
            }
        }
        SymbolKind::Section(i) => {
            let section = &doc.sections[i];
            for (j, task) in doc.tasks.iter().enumerate() {
                if task.line > section.line
                    && task.line < section.end_line
                    && !doc.tasks.iter().any(|t| t.parent == Some(j))
                {
                    add(
                        Symbol {
                            path: symbol.path.clone(),
                            kind: SymbolKind::Task(j),
                        },
                        task.checkbox,
                    );
                }
            }
        }
        SymbolKind::Column(t, c) => add(
            Symbol {
                path: symbol.path.clone(),
                kind: SymbolKind::Definition(doc.tables[t].definition),
            },
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
pub(crate) fn nodes(ws: &Workspace) -> Vec<Symbol> {
    ws.documents
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
                .map(|kind| Symbol {
                    path: path.clone(),
                    kind,
                })
                .collect::<Vec<_>>()
        })
        .collect()
}
pub fn ranges(ws: &Workspace, path: &Path, spans: &[Span]) -> Vec<Range> {
    let text = &ws.documents[path].text;
    spans.iter().map(|s| s.range(text)).collect()
}
