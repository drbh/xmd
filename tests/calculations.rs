use chrono::{DateTime, FixedOffset};
use std::{collections::BTreeMap, path::Path};
use tower_lsp::lsp_types::*;
use wtf::{
    document::Document,
    engine::{Currency, Value},
    highlighting::{TOKEN_TYPES, semantic_tokens},
    intelligence,
    workspace::Workspace,
};

fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-09-16T14:00:00-04:00").unwrap()
}
fn path() -> &'static Path {
    Path::new("/notes/test.wtf")
}
fn note(source: &str) -> Workspace {
    Workspace {
        roots: vec!["/notes".into()],
        documents: [(path().to_path_buf(), Document::parse(source.into()))].into(),
        cache: BTreeMap::new(),
        lookups: Default::default(),
        modules: Default::default(),
    }
}
fn labels(ws: &Workspace) -> Vec<(u32, u32, String)> {
    wtf::RequestContext::new(ws, now())
        .hints(
            path(),
            Range::new(Position::new(0, 0), Position::new(50, 0)),
        )
        .hints
        .into_iter()
        .map(|h| match h.label {
            InlayHintLabel::String(s) => (h.position.line, h.position.character, s),
            other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
fn bracketed_expressions_in_prose_are_calculations_shown_in_place() {
    let source = "[$3,000]:budget\n[$2,444]:spent\n[remaining] := budget - spent\nWe have [remaining] left, which is [remaining / budget] of the budget, or [budget * 2] doubled.\nLunch is $25 and [$25] stays prose; [today()] is a call.\n";
    let ws = note(source);
    let doc = &ws.documents[path()];
    assert_eq!(
        doc.calculations
            .iter()
            .map(|c| c.source.as_str())
            .collect::<Vec<_>>(),
        ["remaining / budget", "budget * 2", "today()"]
    );
    let hints = labels(&ws);
    assert!(hints.contains(&(3, 19, "$556".into())), "{hints:?}");
    let line3 = source.lines().nth(3).unwrap();
    let after = |needle: &str| (line3.find(needle).unwrap() + needle.len()) as u32;
    assert!(
        hints.contains(&(3, after("[remaining / budget]"), "18.5333%".into())),
        "{hints:?}"
    );
    assert!(
        hints.contains(&(3, after("[budget * 2]"), "$6,000".into())),
        "{hints:?}"
    );
    assert!(
        hints.iter().any(|(l, _, s)| *l == 4 && s == "2026-09-16"),
        "{hints:?}"
    );
    assert!(
        !hints.iter().any(|(l, _, s)| *l == 4 && s == "$25"),
        "literals are not annotated: {hints:?}"
    );
    assert_eq!(
        wtf::RequestContext::new(&ws, now())
            .diagnostics(path(), false)
            .len(),
        0
    );
    // Names inside a calculation are real references: they highlight and resolve.
    let tokens = semantic_tokens(doc);
    let kinds: Vec<&str> = tokens
        .iter()
        .map(|t| TOKEN_TYPES[t.token_type as usize])
        .collect();
    assert!(kinds.contains(&"wtfPunctuation") && kinds.contains(&"variable"));
    assert!(doc.references.iter().filter(|r| r.name == "budget").count() >= 3);
    let _ = Value::Money(1.0, Currency::USD);
}

#[test]
fn calculations_report_errors_at_the_token_and_hover_with_substitution() {
    let ws = note("[$3,000]:budget\n[30m]:slot\nOops [budget + slot] and [budgett * 2].\n");
    let messages: Vec<_> = wtf::RequestContext::new(&ws, now())
        .diagnostics(path(), false)
        .into_iter()
        .map(|d| (d.range.start.character, d.message))
        .collect();
    assert!(
        messages
            .iter()
            .any(|(_, m)| m.contains("Unsupported arithmetic types") || m.contains("Cannot")),
        "{messages:?}"
    );
    assert!(
        messages.iter().any(|(_, m)| m == "Unknown name 'budgett'"),
        "{messages:?}"
    );
    let ws = note("[$3,000]:budget\n[$2,444]:spent\nRatio [spent / budget] here.\n");
    let line = ws.documents[path()].line(2);
    let slash = line.find('/').unwrap() as u32;
    let hover = wtf::RequestContext::new(&ws, now())
        .calculation_hover(path(), Position::new(2, slash))
        .unwrap();
    let text = match hover.contents {
        HoverContents::Markup(m) => m.value,
        other => panic!("{other:?}"),
    };
    assert!(text.starts_with("**81.4667% · Ratio**"), "{text}");
    assert!(
        text.contains("spent / budget\n= $2,444 / $3,000\n= 81.4667%"),
        "{text}"
    );
    // Hovering a name inside the brackets is still that name's hover.
    let (symbol, _) = intelligence::symbol_at(
        &ws,
        path(),
        Position::new(2, line.find("spent").unwrap() as u32 + 1),
    )
    .unwrap();
    assert_eq!(ws.named(&symbol).name, "spent");
}

#[test]
fn a_line_of_math_with_bracketed_variables_shows_its_result() {
    let source = "[$3,000]:budget\n[$2,444]:spent\n[budget] - [spent]\n([budget] - [spent]) / [budget]\n2 + 2\n[budget] * 2 dollars\n- [budget] - 1\n[budget]\n$25\n";
    let ws = note(source);
    let doc = &ws.documents[path()];
    let lines: Vec<(usize, &str, bool)> = doc
        .calculations
        .iter()
        .map(|c| (c.span.line, c.source.as_str(), c.bracketed))
        .collect();
    assert_eq!(
        lines,
        [
            (2, " budget  -  spent ", false),
            (3, "( budget  -  spent ) /  budget ", false),
            (4, "2 + 2", false),
        ],
        "prose with words, list items, lone references and lone literals are not lines of math"
    );
    let hints = labels(&ws);
    assert!(hints.contains(&(2, 18, "= $556".into())), "{hints:?}");
    assert!(hints.contains(&(3, 31, "= 18.5333%".into())), "{hints:?}");
    assert!(hints.contains(&(4, 5, "= 4".into())), "{hints:?}");
    // The bracketed references keep their own value hints inside the line.
    assert!(hints.contains(&(2, 8, "$3,000".into())), "{hints:?}");
    assert_eq!(
        wtf::RequestContext::new(&ws, now())
            .diagnostics(path(), false)
            .len(),
        0
    );
    let hover = wtf::RequestContext::new(&ws, now())
        .calculation_hover(path(), Position::new(2, 10))
        .unwrap();
    let text = match hover.contents {
        HoverContents::Markup(m) => m.value,
        other => panic!("{other:?}"),
    };
    assert!(text.starts_with("**$556 · Money**"), "{text}");
    assert!(
        text.contains("budget - spent\n= $3,000 - $2,444\n= $556"),
        "{text}"
    );
    // Errors point into the line.
    let bad = note("[$3,000]:budget\n[budget] - [nope]\n");
    let issues: Vec<_> = wtf::RequestContext::new(&bad, now())
        .diagnostics(path(), false)
        .into_iter()
        .map(|d| d.message)
        .collect();
    assert_eq!(issues, ["Unknown name 'nope'"]);
}
