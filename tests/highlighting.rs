use jot::{
    document::{Document, byte_at},
    highlighting::{TOKEN_MODIFIERS, TOKEN_TYPES, semantic_tokens},
};

#[derive(Debug)]
struct Token {
    row: usize,
    start: usize,
    end: usize,
    text: String,
    kind: &'static str,
    modifiers: u32,
}
fn tokens(source: &str) -> Vec<Token> {
    let doc = Document::parse(source.into());
    let (mut row, mut column, mut previous_end) = (0, 0, 0);
    semantic_tokens(&doc)
        .into_iter()
        .map(|t| {
            if t.delta_line != 0 {
                column = 0;
                previous_end = 0;
            }
            row += t.delta_line as usize;
            column += t.delta_start;
            assert!(column >= previous_end && t.length > 0);
            let line = doc.line(row);
            let start = byte_at(line, column).expect("UTF-16 start boundary");
            let end = byte_at(line, column + t.length).expect("UTF-16 end boundary");
            previous_end = column + t.length;
            assert!(t.token_modifiers_bitset < 1 << TOKEN_MODIFIERS.len());
            Token {
                row,
                start,
                end,
                text: line[start..end].into(),
                kind: TOKEN_TYPES[t.token_type as usize],
                modifiers: t.token_modifiers_bitset,
            }
        })
        .collect()
}
fn token_at<'a>(tokens: &'a [Token], source: &str, row: usize, text: &str) -> &'a Token {
    let byte = source.lines().nth(row).unwrap().find(text).unwrap();
    tokens
        .iter()
        .find(|t| t.row == row && t.start <= byte && t.end >= byte + text.len())
        .unwrap_or_else(|| panic!("No token for {text:?} at row {row}: {tokens:#?}"))
}
fn assert_kind(source: &str, row: usize, text: &str, kind: &str, modifiers: u32) {
    let all = tokens(source);
    let token = token_at(&all, source, row, text);
    assert_eq!(
        (token.kind, token.modifiers),
        (kind, modifiers),
        "{text}: {token:?}"
    );
}

#[test]
fn types_calls_properties_and_declarations_are_distinct() {
    let source = "# Trip :trip\n🧳 [$3,000]:budget and [2026-09-16]:depart.\n[focus] := countdown(25m)\n[used] := 47%\n[ok] := true\n[label] := \"hello 🦀\"\nHave [budget] and [focus.remaining].\n";
    for (row, text, kind, modifiers) in [
        (0, "#", "jotPunctuation", 0),
        (0, "Trip", "heading", 0),
        (0, "trip", "variable", 1),
        (1, "$3,000", "jotMoney", 0),
        (1, "budget", "variable", 1),
        (1, "2026-09-16", "jotDate", 0),
        (2, "focus", "variable", 1),
        (2, "countdown", "function", 2),
        (2, "25m", "jotDuration", 0),
        (2, ":=", "operator", 0),
        (3, "47%", "jotRatio", 0),
        (4, "true", "jotBoolean", 0),
        (5, "\"hello 🦀\"", "string", 0),
        (6, "budget", "variable", 0),
        (6, "remaining", "property", 0),
        (6, ".", "jotPunctuation", 0),
    ] {
        assert_kind(source, row, text, kind, modifiers);
    }
    let all = tokens(source);
    assert!(
        all.iter()
            .all(|t| !t.text.contains("Have") && !t.text.contains("🧳"))
    );
}

#[test]
fn table_headers_and_scoped_columns_share_property_colors() {
    let source = "[fruit] := table\n| item | quantity | price | today |\n| --- | ---: | ---: | --- |\n| \"apple|pear 🦀\" | 2 | $3.30 | 2026-09-16 |\n[total] := sum(fruit, quantity * price)\n[next] := sum(fruit, today + 1d\n";
    for (row, text, kind, modifiers) in [
        (0, "table", "keyword", 0),
        (1, "quantity", "property", 1),
        (1, "|", "jotPunctuation", 0),
        (2, "---:", "jotPunctuation", 0),
        (3, "\"apple|pear 🦀\"", "string", 0),
        (3, "2", "number", 0),
        (3, "$3.30", "jotMoney", 0),
        (3, "2026-09-16", "jotDate", 0),
        (4, "sum", "function", 2),
        (4, "fruit", "variable", 0),
        (4, "quantity", "property", 0),
        (4, "price", "property", 0),
        (4, "*", "operator", 0),
        (5, "today", "property", 0),
    ] {
        assert_kind(source, row, text, kind, modifiers);
    }
}

#[test]
fn checklists_metadata_links_and_code_have_readable_hierarchy() {
    let source = "- [x] Pack passport :pack @due(tomorrow) @estimate(30m)\n- [ ] Review @after(pack) @tag(trip)\nMeet @at(2026-09-16T15:00:00-04:00)\n[docs](https://example.com) and [https://example.com]:site\nInline `[bogus] := countdown(3m)` stays inert.\n```jot\n[bogus] := countdown(3m)\n```\n<!-- @due(tomorrow) [bogus] -->\n";
    for (row, text, kind, modifiers) in [
        (0, "[x]", "jotCheckboxChecked", 0),
        (0, "Pack passport", "jotTaskDone", 0),
        (0, "pack", "variable", 1),
        (0, "@due", "decorator", 0),
        (0, "tomorrow", "jotDate", 0),
        (0, "30m", "jotDuration", 0),
        (1, "[ ]", "jotCheckbox", 0),
        (1, "@after", "decorator", 0),
        (1, "pack", "variable", 0),
        (1, "trip", "string", 0),
        (2, "2026-09-16T15:00:00-04:00", "jotDate", 0),
        (3, "docs", "jotLink", 0),
        (3, "https://example.com", "jotLink", 0),
        (4, "countdown", "jotCode", 0),
        (6, "countdown", "jotCode", 0),
        (8, "@due", "comment", 0),
    ] {
        assert_kind(source, row, text, kind, modifiers);
    }
    let all = tokens(source);
    assert!(all.iter().all(|t| !t.text.contains("Review")));
}

#[test]
fn unicode_crlf_empty_and_malformed_documents_produce_valid_nonoverlapping_tokens() {
    for source in [
        "",
        "\r\n",
        "🦀 [\"é𐐀\"]:name\r\nHave [name].",
        "[x] := sum(fruit, quantity +",
        "[x] := \"unfinished",
        "[t] := table\n|a|b|\n",
        "- [x] 🦀 `literal`\n",
    ] {
        tokens(source);
    }
}

#[test]
fn zed_rules_cover_the_shared_legend_and_match_browser_palette() {
    let rules: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../zed-extension/languages/jot/semantic_token_rules.json"
    ))
    .unwrap();
    let browser = include_str!("../web/editor.js");
    for kind in TOKEN_TYPES {
        let rule = rules
            .iter()
            .find(|r| r["token_type"] == *kind && r.get("token_modifiers").is_none())
            .unwrap();
        let color = rule["foreground_color"]
            .as_str()
            .unwrap()
            .trim_start_matches('#');
        assert!(
            browser.contains(&format!("token: \"{kind}\", foreground: \"{color}\"")),
            "Missing browser style for {kind}"
        );
    }
    for kind in ["variable", "property"] {
        let first = rules.iter().find(|r| r["token_type"] == kind).unwrap();
        assert_eq!(first["token_modifiers"], serde_json::json!(["declaration"]));
        assert_eq!(first["font_weight"], "bold");
    }
}

#[test]
fn raw_urls_and_paths_are_highlighted_without_swallowing_prose_punctuation() {
    let source = "🦀 https://example.com/a_(b), ../src/main.rs and ~/.config/zed/settings.json.\r\nREADME.md and [today].";
    for target in [
        "https://example.com/a_(b)",
        "../src/main.rs",
        "~/.config/zed/settings.json",
    ] {
        assert_kind(source, 0, target, "jotLink", 0);
    }
    assert_kind(source, 1, "README.md", "jotLink", 0);
    let all = tokens(source);
    assert!(
        all.iter()
            .filter(|t| t.kind == "jotLink")
            .all(|t| !t.text.ends_with([',', '.']))
    );
}

#[test]
fn recognizable_prose_values_and_heading_times_are_highlighted() {
    let source = "# 9:38 AM\r\n🦀 Interview 09/17/2026 10:00 AM – 10:45 AM.\r\nStart 7AM; return 14:30 or 11:59:59pm.\r\nTomorrow: pay ($3,000), allow 30m, finish 80% with 2 copies.\r\nMeet next Monday; confirm true or false.\r\nOn 2026-09-17, timestamp 2026-09-17T10:00:00-04:00.\r\nNumbers -3.5 and +.25; durations 1.5h and -30s; refund -$25.\r\n";
    for (row, text, kind) in [
        (0, "9:38 AM", "jotTime"),
        (1, "09/17/2026", "jotDate"),
        (1, "10:00 AM", "jotTime"),
        (1, "10:45 AM", "jotTime"),
        (2, "7AM", "jotTime"),
        (2, "14:30", "jotTime"),
        (2, "11:59:59pm", "jotTime"),
        (3, "Tomorrow", "jotDate"),
        (3, "$3,000", "jotMoney"),
        (3, "30m", "jotDuration"),
        (3, "80%", "jotRatio"),
        (3, "2", "number"),
        (4, "next Monday", "jotDate"),
        (4, "true", "jotBoolean"),
        (4, "false", "jotBoolean"),
        (5, "2026-09-17", "jotDate"),
        (5, "2026-09-17T10:00:00-04:00", "jotDate"),
        (6, "-3.5", "number"),
        (6, "+.25", "number"),
        (6, "1.5h", "jotDuration"),
        (6, "-30s", "jotDuration"),
        (6, "-$25", "jotMoney"),
    ] {
        assert_kind(source, row, text, kind, 0);
    }
    let doc = Document::parse(source.into());
    assert!(doc.definitions.is_empty() && doc.tasks.is_empty() && doc.events.is_empty());
}

#[test]
fn prose_recognition_respects_inert_text_symbols_links_and_invalid_values() {
    let source = "Invalid 02/30/2026 2026-02-30 25:00 13PM 0AM 9:99 AM 3/4 v1.2 1.2.3 item30m day2026-09-17.\n`09/17/2026 7AM $20 true`\n<!-- Tomorrow 09/17/2026 -->\n```text\n30m 7AM 09/17/2026\n```\n[2]:tomorrow\n[future] := tomorrow + 2\n[09/17/2026]:label\nhttps://example.com/2026-09-17?q=30m\n[future] and ./2026-09-17.txt\n";
    let all = tokens(source);
    assert!(
        all.iter().all(|t| t.row != 0),
        "Invalid values should not get partial matches: {all:#?}"
    );
    for (row, text, kind, modifiers) in [
        (1, "7AM", "jotCode", 0),
        (2, "Tomorrow", "comment", 0),
        (4, "30m", "jotCode", 0),
        (6, "tomorrow", "variable", 1),
        (7, "tomorrow", "variable", 0),
        (8, "09/17/2026", "string", 0),
        (9, "2026-09-17", "jotLink", 0),
        (10, "2026-09-17", "jotLink", 0),
    ] {
        assert_kind(source, row, text, kind, modifiers);
    }
}

#[test]
fn checkbox_state_is_distinct_from_the_completed_task_title() {
    for source in [
        "- [ ]",
        "  * [ ] Read",
        "+ [x] Read",
        "- [X] Read\n  - [ ] Child",
    ] {
        let all = tokens(source);
        for task in &Document::parse(source.into()).tasks {
            let text =
                &source.lines().nth(task.line).unwrap()[task.checkbox.start..task.checkbox.end];
            let checkbox = token_at(&all, source, task.line, text);
            assert_eq!(
                checkbox.text, text,
                "Checkbox must have its own token, not merge into the title"
            );
            assert_eq!(
                checkbox.kind,
                if task.checked {
                    "jotCheckboxChecked"
                } else {
                    "jotCheckbox"
                }
            );
        }
    }
}
