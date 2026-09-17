use chrono::{DateTime, FixedOffset};
use jot::{
    charts,
    document::Document,
    engine::Value,
    intelligence, presentation,
    workspace::{Symbol, SymbolKind, Workspace},
};
use std::{collections::BTreeMap, path::Path};
use tower_lsp::lsp_types::*;

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
        lookups: Default::default(),
    }
}
fn def(i: usize) -> Symbol {
    Symbol {
        path: path().into(),
        kind: SymbolKind::Definition(i),
    }
}

#[test]
fn bars_and_sparklines_are_plain_text() {
    assert_eq!(charts::bar(3, 5), "██████░░░░ 60%");
    assert_eq!(charts::bar(0, 0), "░░░░░░░░░░ 0%");
    assert_eq!(charts::bar(7, 7), "██████████ 100%");
    assert_eq!(charts::sparkline(&[1.0, 2.0, 3.0, 8.0]), "▁▂▃█");
    assert_eq!(charts::sparkline(&[4.0, 4.0]), "▅▅");
    assert_eq!(charts::sparkline(&[]), "");
    assert_eq!(
        charts::series(&[
            Value::Money(3.0, jot::engine::Currency::USD),
            Value::Money(9.0, jot::engine::Currency::USD),
            Value::Money(6.0, jot::engine::Currency::USD)
        ])
        .unwrap(),
        "`▁█▅` $3 → $9"
    );
    assert_eq!(
        charts::series(&[Value::Text("a".into()), Value::Number(1.0)]),
        None
    );
}

#[test]
fn column_and_sum_hovers_include_sparklines() {
    let ws = ws(
        "[t] := table\n| item | qty | price |\n|---|---|---|\n| apple | 2 | $3 |\n| pear | 6 | $1 |\n| fig | 4 | $2 |\n[total] := sum(t, qty * price)\n",
    );
    let qty = intelligence::hover(
        &ws,
        &Symbol {
            path: path().into(),
            kind: SymbolKind::Column(0, 1),
        },
        now(),
    );
    assert!(qty.contains("`▁█▅` 2 → 6"), "{qty}");
    let item = intelligence::hover(
        &ws,
        &Symbol {
            path: path().into(),
            kind: SymbolKind::Column(0, 0),
        },
        now(),
    );
    assert!(!item.contains('▁'), "text columns have no chart: {item}");
    let total = intelligence::hover(&ws, &def(1), now());
    assert!(
        total.contains("Row contributions:\n\n`▁▁█` $6 → $8\n"),
        "{total}"
    );
}

#[test]
fn countdowns_and_checklists_show_progress_bars() {
    let source = "[focus] := countdown(10m, 4m)\n# Plan :plan\n- [x] a\n- [ ] b :b @timer(focus)\n  - [x] c\n  - [ ] d\n";
    let ws = ws(source);
    let timer = intelligence::hover(&ws, &def(0), now());
    assert!(timer.contains("`████░░░░░░ 40%`"), "{timer}");
    let plan = intelligence::hover(
        &ws,
        &Symbol {
            path: path().into(),
            kind: SymbolKind::Section(0),
        },
        now(),
    );
    assert!(plan.contains("`███████░░░ 67%` 2/3 complete"), "{plan}");
    let hints = presentation::hints_at(
        &ws,
        path(),
        now(),
        Range::new(Position::new(0, 0), Position::new(20, 0)),
    );
    let label = |line: u32| {
        hints
            .iter()
            .find(|h| h.position.line == line)
            .map(|h| match &h.label {
                InlayHintLabel::String(s) => s.clone(),
                other => panic!("{other:?}"),
            })
            .unwrap()
    };
    assert_eq!(label(0), "= ⏳ ███░░░░░ 06:00 remaining · paused");
    assert_eq!(label(1), "█████░░░ 2/3 complete");
    assert!(
        label(3).starts_with("⏳ ███░░░░░ 06:00 remaining · paused · ████░░░░ 1/2 subtasks"),
        "{}",
        label(3)
    );
    assert_eq!(charts::gauge(0, 0), "░░░░░░░░");
    assert_eq!(charts::gauge(3, 3), "████████");
    let tooltip = |line: u32| {
        hints
            .iter()
            .find(|h| h.position.line == line)
            .map(|h| match &h.tooltip {
                Some(InlayHintTooltip::MarkupContent(m)) => m.value.clone(),
                other => panic!("{other:?}"),
            })
            .unwrap()
    };
    assert_eq!(tooltip(1), "`███████░░░ 67%` 2/3 complete");
    assert!(
        tooltip(3).starts_with("`█████░░░░░ 50%` 1/2 subtasks\n\n"),
        "{}",
        tooltip(3)
    );
}
