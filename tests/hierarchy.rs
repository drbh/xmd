use chrono::{DateTime, FixedOffset};
use jot::{
    document::Document,
    hierarchy,
    workspace::{Symbol, SymbolKind, Workspace},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use tower_lsp::lsp_types::Position;

fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-09-16T14:00:00-04:00").unwrap()
}
fn path() -> &'static Path {
    Path::new("/notes/test.jot")
}
fn ws(source: &str) -> Workspace {
    Workspace {
        roots: vec!["/notes".into()],
        documents: [(path().to_path_buf(), Document::parse(source.into()))].into(),
        cache: BTreeMap::new(),
    }
}
fn point(ws: &Workspace, row: usize, needle: &str) -> Position {
    let line = ws.documents[path()].line(row);
    let end = line.find(needle).unwrap() + needle.len();
    Position::new(row as u32, line[..end].encode_utf16().count() as u32)
}
fn names(ws: &Workspace, edges: Vec<(Symbol, Vec<jot::document::Span>)>) -> Vec<String> {
    edges
        .into_iter()
        .map(|(s, spans)| format!("{}×{}", hierarchy::label(ws, &s), spans.len()))
        .collect()
}

const NOTE: &str = "\
[$3,000]:budget
[$1,410]:spent
[cash] := budget - spent
[half] := cash / 2
[t] := table
| item | qty | price |
|---|---|---|
| apple | 2 | $3 |
[total] := sum(t, qty * price)
# Plan :plan
- [ ] Buy :buy @estimate(30m)
  - [x] Pick :pick
  - [ ] Pay @after(pick)
- [ ] Ship @after(buy, cash > spent)
Prose mentions [cash] without depending on it.
";

#[test]
fn values_report_what_they_read_and_who_reads_them() {
    let ws = ws(NOTE);
    let cash = hierarchy::prepare(&ws, path(), point(&ws, 2, "[ca")).unwrap();
    assert_eq!(cash.kind, SymbolKind::Definition(2));
    assert_eq!(
        names(&ws, hierarchy::dependencies(&ws, &cash)),
        ["budget×1", "spent×1"]
    );
    let mut readers = names(&ws, hierarchy::dependents(&ws, &cash));
    readers.sort();
    // The prose mention is a reference, not a dependency edge.
    assert_eq!(readers, ["Ship×1", "half×1"]);
    // Preparing on a reference resolves to its definition.
    let via_reference = hierarchy::prepare(&ws, path(), point(&ws, 3, "cas")).unwrap();
    assert_eq!(via_reference, cash);
    let item = hierarchy::item(&ws, &cash, now());
    assert_eq!(item.name, "cash");
    assert_eq!(item.detail.as_deref(), Some("Money · $1,590"));
    assert_eq!(hierarchy::decode(&ws, &item), Some(cash));
}

#[test]
fn columns_and_sums_form_edges_through_the_table() {
    let ws = ws(NOTE);
    let total = hierarchy::prepare(&ws, path(), point(&ws, 8, "[tot")).unwrap();
    let mut deps = names(&ws, hierarchy::dependencies(&ws, &total));
    deps.sort();
    assert_eq!(deps, ["price×1", "qty×1", "t×1"]);
    let qty = Symbol {
        path: path().into(),
        kind: SymbolKind::Column(0, 1),
    };
    assert_eq!(names(&ws, hierarchy::dependents(&ws, &qty)), ["total×1"]);
    assert_eq!(names(&ws, hierarchy::dependencies(&ws, &qty)), ["t×1"]);
    let item = hierarchy::item(&ws, &qty, now());
    assert_eq!(item.detail.as_deref(), Some("Number · column of t"));
}

#[test]
fn tasks_link_through_after_subtasks_and_checklists() {
    let ws = ws(NOTE);
    let ship = hierarchy::prepare(&ws, path(), Position::new(13, 3)).unwrap();
    assert_eq!(ship.kind, SymbolKind::Task(3));
    assert_eq!(
        names(&ws, hierarchy::dependencies(&ws, &ship)),
        ["buy×1", "cash×1", "spent×1"]
    );
    let item = hierarchy::item(&ws, &ship, now());
    assert_eq!(item.name, "Ship");
    assert_eq!(item.detail.as_deref(), Some("task · blocked by buy"));
    let buy = hierarchy::prepare(&ws, path(), point(&ws, 10, ":bu")).unwrap();
    assert_eq!(
        names(&ws, hierarchy::dependencies(&ws, &buy)),
        ["pick×1", "Pay×1"]
    );
    let mut readers = names(&ws, hierarchy::dependents(&ws, &buy));
    readers.sort();
    assert_eq!(readers, ["Ship×1"]);
    let plan = hierarchy::prepare(&ws, path(), point(&ws, 9, ":pl")).unwrap();
    let mut leaves = names(&ws, hierarchy::dependencies(&ws, &plan));
    leaves.sort();
    assert_eq!(leaves, ["Pay×1", "Ship×1", "pick×1"]);
    assert_eq!(
        hierarchy::item(&ws, &plan, now()).detail.as_deref(),
        Some("checklist · 1/3 complete")
    );
}

#[test]
fn stale_or_foreign_items_are_rejected() {
    let ws = ws(NOTE);
    let mut item = hierarchy::item(
        &ws,
        &Symbol {
            path: path().into(),
            kind: SymbolKind::Task(3),
        },
        now(),
    );
    item.data = Some(
        serde_json::json!({"path": PathBuf::from("/notes/other.jot"), "kind": "task", "index": 0, "column": 0}),
    );
    assert_eq!(hierarchy::decode(&ws, &item), None);
    item.data = Some(serde_json::json!({"path": path(), "kind": "task", "index": 99, "column": 0}));
    assert_eq!(hierarchy::decode(&ws, &item), None);
    assert_eq!(hierarchy::prepare(&ws, path(), Position::new(14, 0)), None);
}
