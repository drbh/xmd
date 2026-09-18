use chrono::NaiveDate;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    process::Command,
};
use tower_lsp::lsp_types::{Position, Range};
use wtf::{
    actions,
    document::Document,
    editor,
    engine::{Engine, Value},
    resources,
    workspace::Workspace,
};

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 9, 16).unwrap()
}
fn workspace(notes: &[(&str, &str)]) -> Workspace {
    Workspace {
        roots: vec![PathBuf::from("/notes")],
        documents: notes
            .iter()
            .map(|(p, s)| {
                (
                    PathBuf::from(format!("/notes/{p}")),
                    Document::parse((*s).into()),
                )
            })
            .collect(),
        cache: BTreeMap::new(),
        lookups: Default::default(),
        modules: Default::default(),
    }
}
fn evaluate(ws: &Workspace, name: &str) -> Value {
    Engine::new(ws, today())
        .named(Path::new("/notes/daily.wtf"), name)
        .unwrap()
}
fn replace(ws: &mut Workspace, path: &Path, edits: &[tower_lsp::lsp_types::TextEdit]) {
    let next = actions::apply_edits(&ws.documents[path].text, edits).unwrap();
    ws.documents.insert(path.into(), Document::parse(next));
}

#[test]
fn parser_distinguishes_markdown_tasks_code_and_multiple_literals() {
    let ws = workspace(&[(
        "daily.wtf",
        "# Heading\nBudget [$3,000]:budget. Spent [$1,410]:spent.\n[remaining] := budget-spent\n- [ ] Review [remaining] @estimate(20m)\n[Website](https://example.com) ![pic](./image.png)\n`[ignored] := 44`\n```wtf\n[also_ignored] := 9\n```\n<!-- [hidden] := 5 -->\n[日本語]:label\n",
    )]);
    let doc = &ws.documents[Path::new("/notes/daily.wtf")];
    assert_eq!(doc.definitions.len(), 4);
    assert_eq!(doc.tasks.len(), 1);
    assert_eq!(doc.links.len(), 2);
    assert_eq!(evaluate(&ws, "remaining").display(), "$1,590");
    assert_eq!(evaluate(&ws, "label").display(), "日本語");
    assert!(editor::problems(&ws, Path::new("/notes/daily.wtf"), today()).is_empty());
}
#[test]
fn expressions_have_precedence_dates_durations_and_boolean_properties() {
    let ws = workspace(&[(
        "daily.wtf",
        "[n] := 2+3*4\n[grouped] := (2+3)*4\n[division] := 10 / 2\n[ratio] := $60 / $100\n[2026-09-25]:departure\n[due] := departure-7d\n[time] := 30m*2+1h\n[ok] := n == 14 && grouped > n\n[short] := false && missing.merged\n",
    )]);
    for (name, expected) in [
        ("n", "14"),
        ("grouped", "20"),
        ("division", "5"),
        ("ratio", "60%"),
        ("due", "2026-09-18"),
        ("time", "2h"),
        ("ok", "true"),
        ("short", "false"),
    ] {
        assert_eq!(evaluate(&ws, name).display(), expected);
    }
    let mut engine = Engine::new(&ws, today());
    assert!(engine.eval(Path::new("/notes/daily.wtf"), "1/0").is_err());
    assert!(
        engine
            .eval(Path::new("/notes/daily.wtf"), "2026-09-25 + 1h")
            .is_err()
    );
}
#[test]
fn forward_references_cross_files_and_cycles() {
    let ws = workspace(&[
        (
            "daily.wtf",
            "[remaining] := import(\"./resources.wtf\").budget - spent\n[spent] := 1410\n[a] := b\n[b] := a\n",
        ),
        ("resources.wtf", "[$3,000]:budget\n"),
    ]);
    assert_eq!(evaluate(&ws, "remaining").display(), "$1,590");
    assert!(
        Engine::new(&ws, today())
            .named(Path::new("/notes/daily.wtf"), "a")
            .unwrap_err()
            .contains("cycle")
    );
    let ws = workspace(&[
        ("daily.wtf", "Use [budget]."),
        ("one.wtf", "[10]:budget"),
        ("two.wtf", "[20]:budget"),
    ]);
    assert!(
        Engine::new(&ws, today())
            .named(Path::new("/notes/daily.wtf"), "budget")
            .unwrap_err()
            .contains("Unknown name")
    );
}
#[test]
fn checklist_counts_leaves_and_completes_hierarchy() {
    let path = Path::new("/notes/daily.wtf");
    let mut ws = workspace(&[(
        "daily.wtf",
        "## Release :release\n- [ ] Parent\n  - [x] First\n  - [ ] Second @estimate(30m)\n- [ ] Third @estimate(10m)\n- [ ] Fourth @estimate(20m)\n[progress] := completed(release)/total(release)\n[work] := effort(release)\n",
    )]);
    assert_eq!(evaluate(&ws, "progress").display(), "25%");
    assert_eq!(evaluate(&ws, "work").display(), "1h");
    let edits = actions::toggle_task(&ws, path, 0, today()).unwrap();
    replace(&mut ws, path, &edits);
    assert_eq!(evaluate(&ws, "progress").display(), "50%");
    assert_eq!(evaluate(&ws, "work").display(), "30m");
    let hints = editor::hints(
        &ws,
        path,
        today(),
        Range::new(Position::new(0, 0), Position::new(0, 100)),
    );
    assert_eq!(hints.len(), 1);
    assert!(
        serde_json::to_string(&hints)
            .unwrap()
            .contains("2/4 complete")
    );
    let edits = actions::toggle_task(&ws, path, 0, today()).unwrap();
    replace(&mut ws, path, &edits);
    assert_eq!(evaluate(&ws, "progress").display(), "0%");
}
#[test]
fn task_dependencies_block_and_detect_cycles() {
    let path = Path::new("/notes/daily.wtf");
    let mut ws = workspace(&[(
        "daily.wtf",
        "- [ ] Review :review\n- [ ] Publish @after(review)\n",
    )]);
    assert!(
        actions::toggle_task(&ws, path, 1, today())
            .unwrap_err()
            .contains("Blocked")
    );
    let edits = actions::toggle_task(&ws, path, 0, today()).unwrap();
    replace(&mut ws, path, &edits);
    assert!(actions::toggle_task(&ws, path, 1, today()).is_ok());
    let ws = workspace(&[("daily.wtf", "- [ ] A :a @after(b)\n- [ ] B :b @after(a)\n")]);
    assert!(
        Engine::new(&ws, today())
            .blocked(path, 0)
            .unwrap_err()
            .contains("cycle")
    );
}
#[test]
fn recurrence_preserves_month_end_and_completion_history() {
    let path = Path::new("/notes/daily.wtf");
    let mut ws = workspace(&[(
        "daily.wtf",
        "- [ ] Pay bill :bill @every(month) @due(2026-01-31)\n",
    )]);
    let jan = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();
    let feb = NaiveDate::from_ymd_opt(2026, 2, 28).unwrap();
    let edits = actions::toggle_task(&ws, path, 0, jan).unwrap();
    replace(&mut ws, path, &edits);
    assert!(ws.documents[path].text.contains("@due(2026-02-28)"));
    assert!(ws.documents[path].text.contains("@repeat_from(2026-01-31)"));
    let edits = actions::toggle_task(&ws, path, 0, feb).unwrap();
    replace(&mut ws, path, &edits);
    assert!(ws.documents[path].text.contains("@due(2026-03-31)"));
    assert_eq!(ws.documents[path].text.matches("wtf-history").count(), 2);
    assert!(!ws.documents[path].tasks[0].checked);
    assert_eq!(ws.documents[path].tasks.len(), 1);
    assert!(editor::problems(&ws, path, feb).is_empty());
}
#[test]
fn recurring_unscheduled_task_and_crlf_no_trailing_newline() {
    let path = Path::new("/notes/daily.wtf");
    let mut ws = workspace(&[("daily.wtf", "# Life\r\n- [ ] Walk @every(day)")]);
    let edits = actions::toggle_task(&ws, path, 0, today()).unwrap();
    replace(&mut ws, path, &edits);
    assert!(
        ws.documents[path]
            .text
            .contains("@due(2026-09-17) @repeat_from(2026-09-16)\r\n<!-- wtf-history")
    );
    assert_eq!(ws.documents[path].tasks.len(), 1);
}
#[test]
fn relative_dates_freeze_and_appointments_are_separate() {
    let path = Path::new("/notes/daily.wtf");
    let mut ws = workspace(&[(
        "daily.wtf",
        "- [ ] Call @due(next Friday)\n- [ ] Plan @scheduled(tomorrow)\n- Coffee @at(2026-09-16T14:00-04:00)\n",
    )]);
    assert!(editor::problems(&ws, path, today()).is_empty());
    let edits = actions::freeze_dates(&ws, path, today());
    assert_eq!(edits.len(), 2);
    replace(&mut ws, path, &edits);
    assert!(ws.documents[path].text.contains("@due(2026-09-18)"));
    assert!(ws.documents[path].text.contains("@scheduled(2026-09-17)"));
    let ctx = wtf::query::QueryContext::new(
        chrono::DateTime::parse_from_rfc3339("2026-09-16T12:00:00-04:00").unwrap(),
    );
    let entries =
        wtf::query::execute(&ws, &wtf::query::Query::parse("entries").unwrap(), &ctx).unwrap();
    assert_eq!(entries.rows.len(), 3);
    let agenda = wtf::query::execute(
        &ws,
        &wtf::query::Query::parse("import(\"agenda\").between(entries, today(), today())").unwrap(),
        &ctx,
    )
    .unwrap();
    assert_eq!(agenda.rows.len(), 1);
}
#[test]
fn unicode_highlights_and_edits_use_utf16_positions() {
    let path = Path::new("/notes/daily.wtf");
    let mut ws = workspace(&[(
        "daily.wtf",
        "# café 🌴\r\n- [ ] 日本語 🌴 [$30]:cost @due(tomorrow)\r\n[cost_plus] := cost+5\r\n",
    )]);
    let doc = &ws.documents[path];
    let tokens = editor::semantic_tokens(doc);
    let mut line = 0;
    let mut start = 0;
    let mut end = 0;
    for t in tokens {
        if t.delta_line > 0 {
            line += t.delta_line;
            start = t.delta_start;
            end = 0;
        } else {
            start += t.delta_start;
        }
        assert!(start >= end);
        end = start + t.length;
        assert!(end <= doc.line(line as usize).encode_utf16().count() as u32);
    }
    let edits = actions::freeze_dates(&ws, path, today());
    replace(&mut ws, path, &edits);
    assert!(
        ws.documents[path]
            .text
            .contains("日本語 🌴 [$30]:cost @due(2026-09-17)\r\n")
    );
    assert_eq!(evaluate(&ws, "cost_plus").display(), "$35");
}
#[test]
fn resources_keep_definition_origin_through_cross_file_aliases() {
    let ws = workspace(&[
        (
            "daily.wtf",
            "[alias] := import(\"./project/resources.wtf\").receipt\n",
        ),
        ("project/resources.wtf", "[./assets/receipt.png]:receipt\n"),
    ]);
    let Value::Resource(resource) = evaluate(&ws, "alias") else {
        panic!()
    };
    assert_eq!(
        resource
            .url(Path::new("/notes/daily.wtf"))
            .unwrap()
            .as_str(),
        "file:///notes/project/assets/receipt.png"
    );
    assert!(
        resources::Resource::parse("geo:40.73,-73.98")
            .unwrap()
            .url(Path::new("/notes/daily.wtf"))
            .unwrap()
            .as_str()
            .contains("openstreetmap")
    );
    assert!(
        resources::Resource::parse("geo:91,0")
            .unwrap()
            .url(Path::new("/notes/daily.wtf"))
            .is_err()
    );
}
#[test]
fn cached_github_status_is_typed_and_absent_checks_are_unknown() {
    let mut ws = workspace(&[(
        "daily.wtf",
        "[https://github.com/acme/app/pull/42]:pr\n[ready] := pr.merged && pr.checks_passed\n",
    )]);
    assert!(
        Engine::new(&ws, today())
            .named(Path::new("/notes/daily.wtf"), "ready")
            .is_err()
    );
    let now = chrono::DateTime::parse_from_rfc3339("2026-09-16T12:00:00Z")
        .unwrap()
        .to_utc();
    let metadata=resources::metadata("pull",&serde_json::json!({"title":"Ship it","state":"MERGED","mergedAt":"2026-09-16T10:00:00Z","statusCheckRollup":[{"conclusion":"SUCCESS"}]}),now).unwrap();
    assert!(metadata.summary().contains("cached 2026-09-16"));
    ws.cache
        .insert("https://github.com/acme/app/pull/42".into(), metadata);
    assert_eq!(evaluate(&ws, "ready"), Value::Bool(true));
    let empty = resources::metadata(
        "pull",
        &serde_json::json!({"title":"No checks","state":"OPEN","statusCheckRollup":[]}),
        now,
    )
    .unwrap();
    assert!(empty.checks.is_none());
    assert!(resources::github("https://github.com.evil.example/acme/app/pull/42").is_none());
}
#[test]
fn cli_agendas_filter_ignored_notes_without_mutating_them() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::write(root.join(".gitignore"), "ignored.wtf\n").unwrap();
    std::fs::write(root.join("ignored.wtf"), "- [ ] invisible\n").unwrap();
    std::fs::create_dir(root.join("target")).unwrap();
    std::fs::write(root.join("target/build.wtf"), "- [ ] also invisible\n").unwrap();
    let run = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_wtf"))
            .current_dir(&root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    let note = "- [ ] Call dentist #errands @due(2026-09-18)\n";
    std::fs::write(root.join("inbox.wtf"), note).unwrap();
    let tasks: serde_json::Value = serde_json::from_str(&run(&[
        "query",
        "--workspace",
        "tasks | where leaf && !done | sort source.path, source.line | where contains(tags, \"errands\")",
        "--json",
    ]))
    .unwrap();
    assert_eq!(tasks.as_array().unwrap().len(), 1);
    assert_eq!(tasks[0]["source"]["line"], 1);
    let agenda: serde_json::Value = serde_json::from_str(&run(&[
        "query",
        "--workspace",
        "import(\"agenda\").between(entries, today(), today() + 6d)",
        "--on",
        "2026-09-16",
        "--json",
    ]))
    .unwrap();
    assert_eq!(agenda.as_array().unwrap().len(), 1);
    assert_eq!(
        std::fs::read_to_string(root.join("inbox.wtf")).unwrap(),
        note
    );
    run(&[
        "query",
        "--workspace",
        "diagnostics | where severity == \"error\"",
        "--fail-on-match",
    ]);
}

#[test]
#[cfg(unix)]
fn github_refresh_persists_metadata_and_keeps_cache_on_failure() {
    use std::os::unix::fs::PermissionsExt;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let bin = root.join("bin");
    std::fs::create_dir(&bin).unwrap();
    let gh = bin.join("gh");
    std::fs::write(&gh,"#!/bin/sh\nif [ \"${WTF_TEST_FAIL:-}\" = 1 ]; then printf '%s\\n' 'fixture failure' >&2; exit 1; fi\nprintf '%s\\n' '{\"title\":\"Fixture PR\",\"state\":\"MERGED\",\"mergedAt\":\"2026-09-16T12:00:00Z\",\"statusCheckRollup\":[{\"conclusion\":\"SUCCESS\"}]}'\n").unwrap();
    std::fs::set_permissions(&gh, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        root.join("links.wtf"),
        "[https://github.com/acme/app/pull/42]:pr\n[ready] := pr.merged && pr.checks_passed\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_wtf"))
        .current_dir(root)
        .env("PATH", &bin)
        .arg("refresh")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let cache = std::fs::read(root.join(".wtf/cache.json")).unwrap();
    let ws = Workspace::load(vec![root.canonicalize().unwrap()]).unwrap();
    assert_eq!(
        Engine::new(&ws, today())
            .named(&root.canonicalize().unwrap().join("links.wtf"), "ready")
            .unwrap(),
        Value::Bool(true)
    );
    let failed = Command::new(env!("CARGO_BIN_EXE_wtf"))
        .current_dir(root)
        .env("PATH", &bin)
        .env("WTF_TEST_FAIL", "1")
        .arg("refresh")
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert_eq!(std::fs::read(root.join(".wtf/cache.json")).unwrap(), cache);
}

#[test]
fn cli_queries_plans_and_converts_alps_problems() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    std::fs::write(
        root.join("bakery.wtf"),
        "[400]:flour_stock\n[bakery] := maximize($3 * bagels + $1.25 * doughnuts)\n| constraint | expression |\n| --- | --- |\n| flour | 12 * bagels + 6.5 * doughnuts <= flour_stock |\n| bagel_min | bagels >= 12 |\n| doughnut_min | doughnuts >= 14 |\n",
    )
    .unwrap();
    let run = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_wtf"))
            .current_dir(&root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    let report = run(&[
        "query",
        "--workspace",
        "plans | where name == \"bakery\" | select solution",
    ]);
    assert!(report.contains("94.75"), "{report}");
    let json: serde_json::Value = serde_json::from_str(&run(&[
        "query",
        "--workspace",
        "plans | where name == \"bakery\" | select solution",
        "--json",
    ]))
    .unwrap();
    assert_eq!(
        json[0]["objective"],
        serde_json::json!({"type":"money","amount":94.75,"currency":"USD"})
    );
    assert_eq!(json[0]["constraints"][0]["binding"], true);
    let exported = run(&["convert", "bakery.wtf", "--to-alps", "bakery"]);
    let problem: serde_json::Value = serde_json::from_str(&exported).unwrap();
    assert_eq!(
        problem["constraints"][0]["expression"],
        "12 * bagels + 6.5 * doughnuts <= 400"
    );
    std::fs::write(root.join("problem.json"), &exported).unwrap();
    let imported = run(&["convert", "--from-alps", "problem.json"]);
    assert!(
        imported.starts_with("[problem] := maximize(3 * bagels + 1.25 * doughnuts)\n| constraint"),
        "{imported}"
    );
    std::fs::write(root.join("imported.wtf"), &imported).unwrap();
    assert!(
        run(&[
            "query",
            "--workspace",
            "plans | where name == \"problem\" | select solution"
        ])
        .contains("94.75")
    );
    let missing = Command::new(env!("CARGO_BIN_EXE_wtf"))
        .current_dir(&root)
        .args(["convert", "bakery.wtf", "--to-alps", "flour_stock"])
        .output()
        .unwrap();
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("not a plan"));
}
