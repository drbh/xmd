use chrono::{DateTime, FixedOffset};
use lsp_types::{Position, Range};
use std::path::Path;
use wtf::{
    actions, diagnostics,
    document::{Document, Span},
    engine::{Engine, Value},
    intelligence, tables,
    workspace::{SymbolKind, Workspace},
};
const SOURCE: &str = "[groceries] := table\n| item | quantity | price |\n| --- | --- | --- |\n| apple | 2 | $3.30 |\n| pear | 4 | $4.30 |\n\n[total] := sum(groceries, quantity * price)\n[units] := sum(groceries, quantity)\n[average] := total / units\nCost [total].\n";
fn path() -> &'static Path {
    Path::new("/notes/test.wtf")
}
fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-09-16T14:00:00-04:00").unwrap()
}
fn ws(source: &str) -> Workspace {
    Workspace {
        roots: vec!["/notes".into()],
        documents: [(path().into(), Document::parse(source.into()))].into(),
        cache: Default::default(),
        lookups: Default::default(),
    }
}
fn span(doc: &Document, row: usize, needle: &str) -> Span {
    let start = doc.line(row).find(needle).unwrap();
    Span::new(row, start, start + needle.len())
}

#[test]
fn tables_evaluate_typed_row_formulas_and_reactive_totals() {
    let ws = ws(SOURCE);
    let mut engine = Engine::at(&ws, now());
    assert_eq!(engine.named(path(), "total").unwrap().display(), "$23.80");
    assert_eq!(engine.named(path(), "units"), Ok(Value::Number(6.0)));
    assert_eq!(engine.named(path(), "average").unwrap().display(), "$3.97");
    assert!(diagnostics::collect(&ws, path(), now().date_naive(), now(), true).is_empty());
    let changed = SOURCE
        .replace("| pear | 4", "| pear | 5")
        .replace("\n\n[total]", "\n| peach | 1 | $2.00 |\n\n[total]");
    let updated = super_workspace(&changed);
    assert_eq!(
        Engine::at(&updated, now())
            .named(path(), "total")
            .unwrap()
            .display(),
        "$30.10"
    );
    let hints = wtf::presentation::hints_at(
        &ws,
        path(),
        now(),
        Range::new(Position::new(0, 0), Position::new(100, 0)),
    );
    assert!(hints.iter().any(|h| h.position.line == 9
        && matches!(&h.label, lsp_types::InlayHintLabel::String(s) if s == "$23.80")));
}
fn super_workspace(source: &str) -> Workspace {
    ws(source)
}

#[test]
fn columns_are_scoped_even_with_same_named_globals_and_other_tables() {
    let source = format!(
        "{SOURCE}\n[999]:price\n[other] := table\n| quantity | price |\n| --- | --- |\n| 3 | $1.00 |\n[nested] := sum(groceries, quantity * sum(other, price))\n"
    );
    let ws = ws(&source);
    assert_eq!(
        Engine::at(&ws, now())
            .named(path(), "nested")
            .unwrap()
            .display(),
        "$6"
    );
    let doc = &ws.documents[path()];
    let reference = doc
        .references
        .iter()
        .find(|r| r.span.line == 6 && r.name == "price")
        .unwrap();
    let symbol = tables::resolve_reference(&ws, path(), reference).unwrap();
    assert_eq!(symbol.kind, SymbolKind::Column(0, 2));
    assert!(tables::validate_rename(&ws, &symbol, "quantity").is_err());
    assert!(tables::validate_rename(&ws, &symbol, "unit_price").is_ok());
    assert!(tables::validate_rename(&ws, &symbol, "true").is_err());
    assert!(diagnostics::collect(&ws, path(), now().date_naive(), now(), true).is_empty());
}

#[test]
fn table_aliases_and_columns_resolve_across_notes_without_capturing_globals() {
    let mut ws = ws("[alias] := groceries\n[cost] := sum(alias, quantity * price)\n");
    ws.documents
        .insert("/notes/data.wtf".into(), Document::parse(SOURCE.into()));
    assert_eq!(
        Engine::at(&ws, now())
            .named(path(), "cost")
            .unwrap()
            .display(),
        "$23.80"
    );
    let reference = ws.documents[path()]
        .references
        .iter()
        .find(|r| r.name == "price")
        .unwrap();
    let symbol = tables::resolve_reference(&ws, path(), reference).unwrap();
    assert_eq!(symbol.path, Path::new("/notes/data.wtf"));
    assert_eq!(symbol.kind, SymbolKind::Column(0, 2));
}

#[test]
fn table_diagnostics_pinpoint_bad_cells_and_unknown_columns() {
    let source = SOURCE
        .replace("| pear | 4", "| pear | four")
        .replace("quantity * price)", "quantity * prcie)");
    let ws = ws(&source);
    let doc = &ws.documents[path()];
    let issues = diagnostics::collect(&ws, path(), now().date_naive(), now(), false);
    assert!(
        issues
            .iter()
            .any(|d| d.range == span(doc, 4, "four").range(&doc.text)
                && d.message.contains("expects Number, found Text")),
        "{issues:?}"
    );
    assert!(
        issues
            .iter()
            .any(|d| d.range == span(doc, 6, "prcie").range(&doc.text)
                && d.message.contains("Unknown column")),
        "{issues:?}"
    );
    for malformed in [
        "| pear | | $4.30 |",
        "| pear | 4 |",
        "| pear | 4 | $4.30 | extra |",
        "| pear | 4 | $oops |",
    ] {
        let source = SOURCE.replace("| pear | 4 | $4.30 |", malformed);
        let ws = super_workspace(&source);
        assert!(!ws.documents[path()].problems.is_empty(), "{malformed}");
        assert!(Engine::at(&ws, now()).named(path(), "total").is_err());
        assert!(tables::formatting(&ws.documents[path()]).is_empty());
    }
}

#[test]
fn table_errors_and_empty_aggregates_are_explicit() {
    for source in [
        "[t] := table\n",
        "[t] := table\n| a | a |\n|---|---|\n|1|2|\n",
        "[t] := table\n| true |\n|---|\n|1|\n",
        "[t] := table\n| a |\n|not separator|\n|1|\n",
    ] {
        let ws = ws(source);
        assert!(
            Engine::at(&ws, now()).named(path(), "t").is_err(),
            "{source}"
        );
    }
    let ws = ws("[t] := table\n| value |\n| --- |\n");
    let mut engine = Engine::at(&ws, now());
    assert!(
        engine
            .eval(path(), "sum(t, value)")
            .unwrap_err()
            .contains("empty table")
    );
    assert!(engine.eval(path(), "sum(12, value)").is_err());
    assert!(engine.eval(path(), "sum(t)").is_err());
}

#[test]
fn quoted_pipes_unicode_crlf_and_ordinary_markdown_survive_formatting() {
    let source = "# Shopping\r\n[t] := table\r\n|item|n|\r\n|:---:|---:|\r\n|\"梨 | pear\"|2|\r\n|apple\\|pear|4|\r\n\r\n| ordinary | table |\r\n|---|---|\r\n|leave|alone|";
    let doc = Document::parse(source.into());
    assert_eq!(doc.tables.len(), 1);
    assert!(doc.problems.is_empty(), "{:?}", doc.problems);
    let formatted = actions::apply_edits(source, &tables::formatting(&doc)).unwrap();
    assert!(formatted.contains("\"梨 | pear\""));
    assert!(formatted.contains("apple\\|pear"));
    assert!(formatted.ends_with("| ordinary | table |\r\n|---|---|\r\n|leave|alone|"));
    assert!(
        tables::formatting(&Document::parse(formatted.clone())).is_empty(),
        "{formatted}"
    );
    let updated = ws(&formatted);
    assert_eq!(
        Engine::at(&updated, now()).eval(path(), "sum(t, n)"),
        Ok(Value::Number(6.0))
    );
    let inert = Document::parse(
        "```wtf\n[t] := table\n|n|\n|---|\n|1|\n```\n<!--\n[t] := table\n-->\n".into(),
    );
    assert!(inert.tables.is_empty());
}

#[test]
fn lsp_column_intelligence_works_for_incomplete_formulas_and_table_symbols() {
    let ws = ws(&SOURCE.replace("quantity * price)", "quantity * pr"));
    let doc = &ws.documents[path()];
    let position = doc.line_end(6);
    let items = intelligence::completions(&ws, path(), position, now(), false);
    assert_eq!(
        items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(),
        ["item", "quantity", "price"]
    );
    assert!(items[2].detail.as_ref().unwrap().contains("Money"));
    let signature = intelligence::signature(doc, position).unwrap();
    assert!(signature.signatures[0].label.contains("sum("));
    assert_eq!(signature.active_parameter, Some(1));
    let position = span(doc, 1, "price").range(&doc.text).start;
    let (symbol, _) = intelligence::symbol_at(&ws, path(), position).unwrap();
    assert!(intelligence::hover(&ws, &symbol, now()).contains("Column of `groceries`"));
    let symbols = wtf::symbols::document_symbols(&ws, path(), now());
    assert_eq!(symbols[0].name, "groceries");
    assert_eq!(
        symbols[0]
            .children
            .as_ref()
            .unwrap()
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["item", "quantity", "price"]
    );
}

#[test]
fn hovers_explain_cells_and_row_contributions_and_refactors_keep_row_scope() {
    let ws = ws(SOURCE);
    let doc = &ws.documents[path()];
    let total = ws.resolve(path(), "total").unwrap();
    let hover = intelligence::hover(&ws, &total, now());
    assert!(
        hover.contains("Row 1: $6.60") && hover.contains("Row 2: $17.20"),
        "{hover}"
    );
    let cell = intelligence::cell_hover(&ws, path(), span(doc, 3, "$3.30").range(&doc.text).start)
        .unwrap();
    assert!(
        serde_json::to_string(&cell)
            .unwrap()
            .contains("groceries.price · Money")
    );
    let range = span(doc, 6, "quantity * price").range(&doc.text);
    assert!(wtf::refactor::actions_for(&ws, path(), range, now()).is_empty());
}

#[test]
fn sums_preserve_units_reject_non_numeric_results_and_limit_nested_work() {
    for (a, b, expected) in [
        ("$3.30", "-$1.10", "$2.20"),
        ("10%", "20%", "30%"),
        ("30m", "1h", "90m"),
        ("1", "2", "3"),
    ] {
        let ws = ws(&format!("[t] := table\n| value |\n|---|\n|{a}|\n|{b}|\n"));
        assert_eq!(
            Engine::at(&ws, now())
                .eval(path(), "sum(t, value)")
                .unwrap()
                .display(),
            expected
        );
    }
    let text = ws("[t] := table\n| value |\n|---|\n|hello|\n");
    assert!(
        Engine::at(&text, now())
            .eval(path(), "sum(t, value)")
            .unwrap_err()
            .contains("found Text")
    );
    let nested = ws(&format!(
        "[t] := table\n| value |\n|---|\n{}",
        "|1|\n".repeat(20)
    ));
    assert!(
        Engine::at(&nested, now())
            .eval(path(), "sum(t, sum(t, sum(t, sum(t, value))))")
            .unwrap_err()
            .contains("200,000 steps")
    );
}

#[test]
fn row_provenance_never_substitutes_globals_and_date_named_columns_are_references() {
    let ws = ws(&format!(
        "{SOURCE}\n[999]:quantity\n[999]:price\n[dates] := table\n|today|tomorrow|\n|---|---|\n|1|2|\n[future] := sum(dates, today + tomorrow)\n"
    ));
    let total = ws.resolve(path(), "total").unwrap();
    let hover = intelligence::hover(&ws, &total, now());
    assert!(!hover.contains("999"), "{hover}");
    let reference = ws.documents[path()]
        .references
        .iter()
        .find(|r| r.name == "tomorrow")
        .unwrap();
    assert!(matches!(
        tables::resolve_reference(&ws, path(), reference)
            .unwrap()
            .kind,
        SymbolKind::Column(1, 1)
    ));
    assert_eq!(
        Engine::at(&ws, now()).named(path(), "future"),
        Ok(Value::Number(3.0))
    );
}

#[test]
fn bracketed_cells_are_calculations_read_from_any_note() {
    let source = "[$10]:unit\n[3]:qty\n[groceries] := table\n| item  | price        |\n| ----- | ------------ |\n| apple | $3.30        |\n| bulk  | [unit * qty] |\n| one   | [unit]       |\n[total] := sum(groceries, price)\n";
    let notes = ws(source);
    let mut engine = Engine::at(&notes, now());
    assert_eq!(
        engine.named(path(), "total").unwrap(),
        Value::Money(43.3, wtf::engine::Currency::USD)
    );
    assert_eq!(
        wtf::diagnostics::collect(&notes, path(), now().date_naive(), now(), false).len(),
        0
    );
    let doc = &notes.documents[path()];
    let cell = &doc.tables[0].rows[1][1];
    assert!(cell.calculated());
    assert_eq!(cell.expression.as_ref().unwrap().0, "unit * qty");
    // References inside the cell resolve, so rename and navigation see them.
    let unit = notes.resolve(path(), "unit").unwrap();
    let uses = wtf::intelligence::occurrences(&notes, &unit);
    assert_eq!(uses.len(), 3, "{uses:?}");
    assert_eq!(uses[1].1, span(doc, 6, "unit"));
    let hover = wtf::intelligence::cell_hover(
        &notes,
        path(),
        Position::new(6, span(doc, 6, "unit").range(&doc.text).start.character),
    )
    .unwrap();
    let text = match hover.contents {
        tower_lsp::lsp_types::HoverContents::Markup(m) => m.value,
        other => panic!("{other:?}"),
    };
    assert!(
        text.contains("Row 2: $30\n\nCalculated from `unit * qty`"),
        "{text}"
    );
    let hints = wtf::presentation::hints_at(
        &notes,
        path(),
        now(),
        Range::new(Position::new(0, 0), Position::new(20, 0)),
    );
    let label = |line: u32| {
        hints
            .iter()
            .find(|h| h.position.line == line)
            .map(|h| match &h.label {
                tower_lsp::lsp_types::InlayHintLabel::String(s) => s.clone(),
                other => panic!("{other:?}"),
            })
    };
    assert_eq!(label(6).as_deref(), Some("$30"));
    assert_eq!(label(7).as_deref(), Some("$10"));
    // A calculated cell of the wrong type is reported at the cell, not the table.
    let wrong = ws(&source.replace("| one   | [unit]       |", "| one   | [qty]        |"));
    let issues = wtf::diagnostics::collect(&wrong, path(), now().date_naive(), now(), false);
    let messages: Vec<_> = issues.iter().map(|d| d.message.as_str()).collect();
    assert!(
        messages.contains(&"Column 'price' expects Money, found Number"),
        "{messages:?}"
    );
    assert_eq!(issues[0].range.start, Position::new(7, 11));
    // Unknown names and empty brackets are ordinary diagnostics.
    let unknown = ws("[t] := table\n| a |\n| --- |\n| [nope] |\n");
    let messages: Vec<_> =
        wtf::diagnostics::collect(&unknown, path(), now().date_naive(), now(), false)
            .into_iter()
            .map(|d| d.message)
            .collect();
    assert_eq!(messages, ["Unknown name 'nope'"]);
    let empty = ws("[t] := table\n| a |\n| --- |\n| [] |\n");
    assert!(
        empty.documents[path()].problems[0]
            .message
            .starts_with("Empty calculation")
    );
    assert!(wtf::tables::formatting(&notes.documents[path()]).is_empty());
}
