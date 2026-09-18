use chrono::{DateTime, FixedOffset};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use tower_lsp::lsp_types::*;
use wtf::{
    actions, diagnostics,
    document::{Document, Span},
    engine::{Engine, Value},
    intelligence, interaction, refactor,
    workspace::Workspace,
};

fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-09-16T14:00:00-04:00").unwrap()
}
fn path() -> &'static Path {
    Path::new("/notes/test.wtf")
}
fn ws(source: &str) -> Workspace {
    Workspace {
        roots: vec!["/notes".into()],
        documents: [(path().to_path_buf(), Document::parse(source.into()))].into(),
        cache: BTreeMap::new(),
        lookups: Default::default(),
        modules: Default::default(),
    }
}
fn point(ws: &Workspace, row: usize, needle: &str) -> Position {
    let line = ws.documents[path()].line(row);
    let end = line.find(needle).unwrap() + needle.len();
    Position::new(row as u32, line[..end].encode_utf16().count() as u32)
}
fn selection(ws: &Workspace, row: usize, text: &str) -> Range {
    let line = ws.documents[path()].line(row);
    let start = line.find(text).unwrap();
    Span::new(row, start, start + text.len()).range(&ws.documents[path()].text)
}
const BASE: &str = "[$3,000]:budget\n[$1,410]:spent\n[remaining_cash] := budget - spent\n[2026-09-25]:departure\n[focus] := countdown(25m)\n[work] := stopwatch()\n[30m]:estimate\n[https://github.com/zed-industries/zed/pull/1]:pr\n# Checklist :checklist\n- [x] Done :done\n";

#[test]
fn prose_value_inlays_respect_unicode_ranges_and_ignore_code_and_links() {
    let mut ws = ws(
        "🦀 [amount] and [amount].\n`[amount]` <!-- [amount] --> [amount](https://example.com)\nMissing [unknown].\n",
    );
    ws.documents.insert(
        PathBuf::from("/notes/values.wtf"),
        Document::parse("[$556]:amount\n".into()),
    );
    let all = wtf::editor::hints_at(
        &ws,
        path(),
        now(),
        Range::new(Position::new(0, 0), Position::new(3, 0)),
    );
    assert_eq!(all.len(), 2);
    assert!(
        all.iter()
            .all(|hint| matches!(&hint.label, InlayHintLabel::String(label) if label == "$556"))
    );
    assert_eq!(all[0].position, point(&ws, 0, "[amount]"));
    let last = ws.documents[path()].line(0).rfind(']').unwrap() + 1;
    assert_eq!(
        all[1].position,
        Span::new(0, last, last)
            .range(&ws.documents[path()].text)
            .start
    );
    assert!(all.iter().all(|h| h.text_edits.is_none()));
    let only_second = wtf::editor::hints_at(
        &ws,
        path(),
        now(),
        Range::new(all[1].position, Position::new(1, 0)),
    );
    assert_eq!(only_second.len(), 1);
    assert_eq!(only_second[0].position, all[1].position);
}

fn complete(tail: &str, prefix: &str) -> Vec<CompletionItem> {
    let ws = ws(&format!("{BASE}{tail}"));
    intelligence::completions(&ws, path(), point(&ws, 10, prefix), now(), true)
}
#[test]
fn completion_is_typed_contextual_and_includes_value_previews() {
    let items = complete("- [ ] Do @timer(", "@timer(");
    let names: Vec<_> = items.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.contains(&"focus") && names.contains(&"work"));
    assert!(items.iter().any(|i| {
        i.detail
            .as_ref()
            .is_some_and(|d| d.contains("Countdown") && d.contains("25:00"))
    }));
    let items = complete("- [ ] Do @due(", "@due(");
    assert!(items.iter().any(|i| i.label == "departure"));
    assert!(items.iter().any(|i| i.label == "tomorrow"));
    assert!(
        !items
            .iter()
            .any(|i| i.label == "budget" || i.label == "work")
    );
    let items = complete("[sum] := effort(", "effort(");
    assert_eq!(
        items.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
        vec!["checklist"]
    );
    let items = complete("[value] := countdown(25m, 0s, ", "countdown(25m, 0s, ");
    assert!(items.iter().any(|i| i.label == "now()"));
    assert!(
        !items
            .iter()
            .any(|i| i.label == "25m" || i.label == "estimate" || i.label == "effort(checklist)")
    );
    let items = complete("- [ ] Do @every(week)", "@every(we");
    assert!(items.iter().any(|i| i.label == "week"));
    assert!(!items.iter().any(|i| i.label == "budget"));
}
#[test]
fn completions_use_exact_replacements_and_negotiate_snippets() {
    let ws = ws(&format!("{BASE}[value] := cou\n"));
    let at = point(&ws, 10, "cou");
    let items = intelligence::completions(&ws, path(), at, now(), true);
    let item = items.iter().find(|i| i.label == "countdown(25m)").unwrap();
    assert_eq!(item.insert_text_format, Some(InsertTextFormat::SNIPPET));
    let Some(CompletionTextEdit::Edit(edit)) = &item.text_edit else {
        panic!()
    };
    assert!(edit.new_text.contains("${1:25m}"));
    assert_eq!(edit.range, selection(&ws, 10, "cou"));
    let items = intelligence::completions(&ws, path(), at, now(), false);
    let item = items.iter().find(|i| i.label == "countdown(25m)").unwrap();
    let Some(CompletionTextEdit::Edit(edit)) = &item.text_edit else {
        panic!()
    };
    assert_eq!(edit.new_text, "countdown(25m)");
    let items = complete("🦀 [value] := pr.sta", "pr.sta");
    assert!(items.iter().any(|i| i.label == "state"));
    assert!(items.iter().any(|i| i.label == "merged"));
    assert!(!items.iter().any(|i| i.label == "elapsed"));
    let items = complete("Use [work.", "work.");
    assert!(items.iter().any(|i| i.label == "elapsed"));
    assert!(!items.iter().any(|i| i.label == "remaining"));
}
#[test]
fn signature_help_tracks_nested_calls_and_ignores_code() {
    let ws = ws("[n] := countdown(25m, 0s, now())\n`countdown(25m)`\n");
    let help = intelligence::signature(&ws.documents[path()], point(&ws, 0, "0s, ")).unwrap();
    assert_eq!(help.active_parameter, Some(2));
    assert!(help.signatures[0].label.contains("started?: DateTime"));
    let help = intelligence::signature(&ws.documents[path()], point(&ws, 0, "now(")).unwrap();
    assert!(help.signatures[0].label.starts_with("now("));
    assert_eq!(help.active_parameter, None);
    assert!(intelligence::signature(&ws.documents[path()], point(&ws, 1, "countdown(")).is_none());
    assert_eq!(
        intelligence::call_context("date(\"next, Friday"),
        Some(("date".into(), 0))
    );
}
#[test]
fn hovers_explain_provenance_and_link_inputs_without_rounding_the_source() {
    let ws = ws(BASE);
    let symbol = ws.resolve(path(), "remaining_cash").unwrap();
    let text = intelligence::hover(&ws, &symbol, now());
    assert!(text.contains("remaining_cash · Money"));
    assert!(text.contains("$3,000 - $1,410"));
    assert!(text.contains("$1,590"));
    assert!(text.contains("[budget](<file:///notes/test.wtf#L1>)"));
}
#[test]
fn extract_preserves_precedence_prose_unicode_and_existing_names() {
    let mut ws = ws("[calculation] := 100\n🦀 Budget $3,000 today.\n[result] := 2 + 3 * 4\n");
    let actions = refactor::actions_for(&ws, path(), selection(&ws, 1, "$3,000"), now());
    let action = actions
        .iter()
        .find(|a| a.title.starts_with("Extract named value"))
        .unwrap();
    let text = actions::apply_edits(&ws.documents[path()].text, &action.edits).unwrap();
    assert!(text.contains("🦀 Budget [$3,000]:amount today."));
    let actions = refactor::actions_for(&ws, path(), selection(&ws, 2, "3 * 4"), now());
    let action = actions
        .iter()
        .find(|a| a.title.starts_with("Extract named calculation"))
        .unwrap();
    assert!(action.title.contains("calculation_1"));
    let text = actions::apply_edits(&ws.documents[path()].text, &action.edits).unwrap();
    ws.documents.insert(path().into(), Document::parse(text));
    assert_eq!(
        Engine::at(&ws, now()).named(path(), "result").unwrap(),
        Value::Number(14.0)
    );
    let ws = self::ws("[result] := 2 + 3 * 4\n");
    assert!(
        !refactor::actions_for(&ws, path(), selection(&ws, 0, "2 + 3"), now())
            .iter()
            .any(|a| a.title.starts_with("Extract"))
    );
}
#[test]
fn inline_guards_cross_file_capture_and_freeze_is_explicit() {
    let mut ws = ws("[base] := 5\n[result] := subtotal * 2\nUse [subtotal].\n");
    ws.documents.insert(
        PathBuf::from("/notes/other.wtf"),
        Document::parse("[base] := 10\n[subtotal] := base + 2\n".into()),
    );
    let range = selection(&ws, 1, "subtotal");
    let choices = refactor::actions_for(&ws, path(), range, now());
    assert!(!choices.iter().any(|a| a.title == "Inline expression"));
    let choices = refactor::actions_for(&ws, path(), selection(&ws, 2, "[subtotal]"), now());
    let freeze = choices
        .iter()
        .find(|a| a.title == "Freeze current value")
        .unwrap();
    assert!(
        actions::apply_edits(&ws.documents[path()].text, &freeze.edits)
            .unwrap()
            .contains("Use 12.")
    );
    let ws = self::ws("[base] := 3 + 4\n[result] := base * 2\n");
    let choices = refactor::actions_for(&ws, path(), selection(&ws, 1, "base"), now());
    let inline = choices
        .iter()
        .find(|a| a.title == "Inline expression")
        .unwrap();
    assert!(
        actions::apply_edits(&ws.documents[path()].text, &inline.edits)
            .unwrap()
            .contains("(3 + 4) * 2")
    );
}
#[test]
fn diagnostic_ranges_point_to_operands_and_do_not_cascade() {
    let ws = ws("[$30]:budget\n[bad] := budget + 5m\n[dependent] := bad * 2\n");
    let ds = diagnostics::collect(&ws, path(), now().date_naive(), now(), true);
    assert_eq!(ds.len(), 1, "{ds:?}");
    assert_eq!(ds[0].range, selection(&ws, 1, "5m"));
    assert!(ds[0].message.contains("Money + Duration"));
    let ws = self::ws("[bad] := unknown +\n[dependent] := bad * 2\n");
    assert!(diagnostics::collect(&ws, path(), now().date_naive(), now(), true).is_empty());
    assert!(!diagnostics::collect(&ws, path(), now().date_naive(), now(), false).is_empty());
}
#[test]
fn unknown_name_fixes_and_ambiguity_locations_are_specific() {
    let ws = ws("[$30]:budget\n🦀 Use [budegt].\n");
    let choices = refactor::actions_for(&ws, path(), selection(&ws, 1, "budegt"), now());
    let fix = choices
        .iter()
        .find(|a| a.title == "Change 'budegt' to 'budget'")
        .unwrap();
    assert_eq!(
        actions::apply_edits(&ws.documents[path()].text, &fix.edits).unwrap(),
        "[$30]:budget\n🦀 Use [budget].\n"
    );
    let create = choices
        .iter()
        .find(|a| a.title.starts_with("Create definition"))
        .unwrap();
    assert!(
        actions::apply_edits(&ws.documents[path()].text, &create.edits)
            .unwrap()
            .contains("[\"TODO\"]:budegt")
    );
    let mut ws = self::ws("Use [amount].\n");
    for file in ["a.wtf", "b.wtf"] {
        ws.documents.insert(
            PathBuf::from(format!("/notes/{file}")),
            Document::parse("[12]:amount".into()),
        );
    }
    let ds = diagnostics::collect(&ws, path(), now().date_naive(), now(), true);
    assert_eq!(ds.len(), 1);
    assert_eq!(ds[0].related_information.as_ref().unwrap().len(), 2);
}
#[test]
fn cycles_have_full_paths_and_related_locations() {
    let ws = ws("[a] := b + 1\n[b] := c + 1\n[c] := a + 1\n");
    let ds = diagnostics::collect(&ws, path(), now().date_naive(), now(), true);
    assert!(ds.iter().any(|d| d.message.contains("a → b → c → a")));
    assert!(
        ds.iter()
            .all(|d| d.related_information.as_ref().is_some_and(|r| r.len() >= 3))
    );
}
#[test]
fn time_dependent_diagnostics_clear_when_clock_changes() {
    let ws = ws("[2026-09-16T14:00:00-04:00]:start\n[rate] := 1s / (now() - start)\n");
    assert!(
        diagnostics::collect(&ws, path(), now().date_naive(), now(), true)
            .iter()
            .any(|d| d.message.contains("Division by zero"))
    );
    assert!(
        diagnostics::collect(
            &ws,
            path(),
            now().date_naive(),
            now() + chrono::Duration::seconds(1),
            true
        )
        .is_empty()
    );
}
#[test]
fn codelenses_are_contextual_and_change_at_timer_expiry() {
    let ws = ws(
        "[tea] := countdown(1s, 0s, 2026-09-16T14:00:00-04:00)\n- [ ] Work @timer(tea)\nNothing here.\n[./receipt.png]:receipt\n",
    );
    let lenses = interaction::lenses(&ws, path(), now());
    assert!(
        lenses
            .iter()
            .any(|l| l.command.as_ref().unwrap().title == "Pause timer 'tea'")
    );
    assert!(
        lenses
            .iter()
            .any(|l| l.command.as_ref().unwrap().title == "Complete task")
    );
    assert!(
        lenses
            .iter()
            .any(|l| l.command.as_ref().unwrap().title == "Open image")
    );
    assert!(!lenses.iter().any(|l| l.range.start.line == 2));
    let lenses = interaction::lenses(&ws, path(), now() + chrono::Duration::seconds(2));
    assert!(
        !lenses
            .iter()
            .any(|l| l.command.as_ref().unwrap().title.starts_with("Pause"))
    );
    assert!(
        lenses
            .iter()
            .any(|l| l.command.as_ref().unwrap().title == "Reset timer 'tea'")
    );
}
#[test]
fn source_literals_roundtrip_types_and_precision_for_freeze() {
    let ws = ws("");
    for value in [
        Value::Number(1.0 / 3.0),
        Value::Money(-123.45, wtf::engine::Currency::USD),
        Value::Ratio(-0.47),
        Value::Duration(73),
        Value::Text("a\"b\n".into()),
        Value::DateTime(now()),
    ] {
        let source = value.source().unwrap();
        assert_eq!(
            Engine::at(&ws, now()).eval(path(), &source).unwrap(),
            value,
            "{source}"
        );
    }
}

#[test]
fn freeze_properties_without_changing_count_types_or_introducing_live_syntax() {
    let ws = ws(
        "[watch] := stopwatch(73s)\n[seconds] := watch.elapsed / 1s\n# Work :work\n- [x] Done\n[count] := completed(work)\n[ratio] := count / total(work)\nDone: [count].\n[text] := \"[watch]\"\nShow [text].\n",
    );
    assert_eq!(
        Engine::at(&ws, now()).named(path(), "text").unwrap(),
        Value::Text("[watch]".into())
    );
    let choices = refactor::actions_for(&ws, path(), selection(&ws, 1, "watch.elapsed"), now());
    let freeze = choices
        .iter()
        .find(|a| a.title == "Freeze current value")
        .unwrap();
    let text = actions::apply_edits(&ws.documents[path()].text, &freeze.edits).unwrap();
    assert!(text.contains("[seconds] := (73s) / 1s"));
    assert!(
        !refactor::actions_for(&ws, path(), selection(&ws, 5, "count"), now())
            .iter()
            .any(|a| a.title == "Freeze current value")
    );
    let choices = refactor::actions_for(&ws, path(), selection(&ws, 6, "[count]"), now());
    let freeze = choices
        .iter()
        .find(|a| a.title == "Freeze current value")
        .unwrap();
    assert!(
        actions::apply_edits(&ws.documents[path()].text, &freeze.edits)
            .unwrap()
            .contains("Done: 1.")
    );
    assert!(
        !refactor::actions_for(&ws, path(), selection(&ws, 8, "[text]"), now())
            .iter()
            .any(|a| a.title == "Freeze current value")
    );
}

#[test]
fn extraction_handles_grouping_and_leaves_metadata_alone() {
    let ws = ws("[n] := (2 + 3) * 4\n- [ ] Repeat @every(2w)\n");
    let choices = refactor::actions_for(&ws, path(), selection(&ws, 0, "2 + 3"), now());
    let extract = choices
        .iter()
        .find(|a| a.title.starts_with("Extract named calculation"))
        .unwrap();
    assert!(
        actions::apply_edits(&ws.documents[path()].text, &extract.edits)
            .unwrap()
            .contains("[n] := (calculation) * 4")
    );
    assert!(
        !refactor::actions_for(&ws, path(), selection(&ws, 1, "2w"), now())
            .iter()
            .any(|a| a.title.starts_with("Extract"))
    );
}

#[test]
fn task_cycles_link_each_task_and_invalid_resources_remain_diagnostic() {
    let ws = ws(
        "- [ ] First :first @after(second)\n- [ ] Second :second @after(first)\n[geo:200,0]:invalid_place\n",
    );
    let ds = diagnostics::collect(&ws, path(), now().date_naive(), now(), true);
    let cycle = ds
        .iter()
        .find(|d| d.message.contains("first → second → first"))
        .unwrap();
    assert_eq!(cycle.related_information.as_ref().unwrap().len(), 3);
    assert!(
        ds.iter()
            .any(|d| d.message.contains("Coordinates are out of range")
                && d.range == selection(&ws, 2, "geo:200,0"))
    );
}
