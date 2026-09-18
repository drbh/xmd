use chrono::{DateTime, FixedOffset};
use lsp_types::DocumentSymbol;
use std::path::Path;
use wtf::{
    document::{Document, Span},
    symbols::document_symbols,
    workspace::Workspace,
};

fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-09-16T14:00:00-04:00").unwrap()
}
fn path() -> &'static Path {
    Path::new("/notes/test.wtf")
}
fn workspace(source: &str) -> Workspace {
    Workspace {
        roots: vec!["/notes".into()],
        documents: [(path().into(), Document::parse(source.into()))].into(),
        cache: Default::default(),
        lookups: Default::default(),
    }
}
fn names(symbols: &[DocumentSymbol]) -> Vec<&str> {
    symbols.iter().map(|s| s.name.as_str()).collect()
}
fn children(symbol: &DocumentSymbol) -> &[DocumentSymbol] {
    symbol.children.as_deref().unwrap_or_default()
}
fn valid_ranges(symbols: &[DocumentSymbol], doc: &Document) {
    for s in symbols {
        assert!(!s.name.trim().is_empty());
        assert!(
            s.range.start <= s.selection_range.start && s.selection_range.end <= s.range.end,
            "{s:?}"
        );
        for p in [
            s.range.start,
            s.range.end,
            s.selection_range.start,
            s.selection_range.end,
        ] {
            assert!((p.line as usize) < doc.text.lines().count());
            assert!(wtf::document::byte_at(doc.line(p.line as usize), p.character).is_some());
        }
        for child in children(s) {
            assert!(
                s.range.start <= child.range.start && child.range.end <= s.range.end,
                "{s:?}"
            );
        }
        valid_ranges(children(s), doc);
    }
}

#[test]
fn symbols_nest_sections_subtasks_values_and_events_in_source_order() {
    let ws = workspace(
        "[1]:before\n# Trip :trip\nOur budget is [$3,000]:budget.\n## Packing\n- [ ] Pack :pack\n  - [x] Passport :passport\n  - [ ] Tickets\n- [ ] Book hotel\n### Travel\nFlight @at(2026-09-20T10:00:00-04:00)\n## Money\n[remaining] := budget - $2,444\n[focus] := countdown(25m)\n# Home\n",
    );
    let result = document_symbols(&ws, path(), now());
    assert_eq!(names(&result), ["before", "Trip", "Home"]);
    let trip = children(&result[1]);
    assert_eq!(names(trip), ["budget", "Packing", "Money"]);
    let packing = children(&trip[1]);
    assert_eq!(names(packing), ["Pack", "Book hotel", "Travel"]);
    assert_eq!(names(children(&packing[0])), ["Passport", "Tickets"]);
    assert_eq!(
        children(&packing[0])[0].detail.as_deref(),
        Some("Complete · :passport")
    );
    assert_eq!(names(children(&packing[2])), ["Flight"]);
    let values = children(&trip[2]);
    assert_eq!(names(values), ["remaining", "focus"]);
    assert_eq!(values[0].detail.as_deref(), Some("Money · $556"));
    assert!(
        values[1]
            .detail
            .as_ref()
            .unwrap()
            .contains("Countdown · ⏳ 25:00")
    );
    assert_eq!(values[0].kind, lsp_types::SymbolKind::VARIABLE);
    assert_eq!(trip[0].kind, lsp_types::SymbolKind::CONSTANT);
    valid_ranges(&result, &ws.documents[path()]);
}

#[test]
fn symbols_preserve_utf16_crlf_and_ignore_prose_references_code_and_comments() {
    let source = "# 🦀 Trip\r\n🦀 [2026-09-20]:departure and [2]:days.\r\n[finish] := departure + 2d   \r\nUse [finish].\r\n`[3]:hidden` <!-- [4]:hidden2 -->\r\n```wtf\r\n# Hidden\r\n[9]:hidden3\r\n```\r\n";
    let ws = workspace(source);
    let result = document_symbols(&ws, path(), now());
    assert_eq!(names(&result), ["🦀 Trip"]);
    let values = children(&result[0]);
    assert_eq!(names(values), ["departure", "days", "finish"]);
    assert_eq!(values[0].range.start.character, 3);
    let line = ws.documents[path()].line(1);
    let start = line.find("departure").unwrap();
    assert_eq!(
        values[0].selection_range,
        Span::new(1, start, start + 9).range(source)
    );
    assert_eq!(values[2].detail.as_deref(), Some("Date · 2026-09-22"));
    valid_ranges(&result, &ws.documents[path()]);
}

#[test]
fn symbols_work_without_headings_and_report_errors_without_disappearing() {
    let mut ws = workspace("[cost] := budget + $10\n[bad] := missing + 1\n- [ ] \n# \n");
    ws.documents.insert(
        "/notes/other.wtf".into(),
        Document::parse("[$90]:budget\n".into()),
    );
    let result = document_symbols(&ws, path(), now());
    assert_eq!(
        names(&result),
        ["cost", "bad", "Untitled task", "Untitled section"]
    );
    assert_eq!(result[0].detail.as_deref(), Some("Money · $100"));
    assert!(result[1].detail.as_ref().unwrap().starts_with("Error · "));
    valid_ranges(&result, &ws.documents[path()]);
    assert!(document_symbols(&workspace(""), path(), now()).is_empty());
    assert!(document_symbols(&workspace("Just prose."), path(), now()).is_empty());
}

#[test]
fn last_definition_with_trailing_spaces_stays_inside_its_section() {
    let ws = workspace("# One\n[answer] := 42  ");
    let result = document_symbols(&ws, path(), now());
    assert_eq!(names(&result), ["One"]);
    assert_eq!(names(children(&result[0])), ["answer"]);
    valid_ranges(&result, &ws.documents[path()]);
}
