#![cfg(feature = "native")]
use chrono::DateTime;
use std::{
    path::Path,
    process::{Command, Output},
};
use wtf::{RequestContext, presentation, workspace::Workspace};

fn run(root: &Path, args: &[&str]) -> Output {
    run_format(root, args, Some("text"))
}
fn run_format(root: &Path, args: &[&str], format: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_wtf"));
    command.current_dir(root).env("TZ", "UTC").args(args);
    if let Some(format) = format {
        command.args(["--format", format]);
    }
    command.output().unwrap()
}
fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn render_cli_matches_editor_hints_with_cross_file_values_modules_and_cached_links() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let notes = root.join("notes");
    let modules = notes.join(".wtf/modules");
    std::fs::create_dir_all(&modules).unwrap();
    let source = "# Work 🦀\r\n- [ ] Ship @estimate(20m)\r\n[total] := price * 2\r\nTotal [total].\r\nhttps://github.com/o/r/pull/42\r\n";
    std::fs::write(notes.join("main.wtf"), source).unwrap();
    std::fs::write(notes.join("values.wtf"), "[$7]:price\n").unwrap();
    let module = "module := {api: 1, id: \"headings\", kind: \"feature\", inputs: {sections: [\"anchor\"]}}\ncollect := fn(ctx) => map(ctx.document.sections, fn(s) => {at: s.anchor, label: \"custom \" + format_date(now(), \"%H:%M\")})\n";
    std::fs::write(modules.join("headings.wtf"), module).unwrap();
    let notes = notes.canonicalize().unwrap();
    let clock = DateTime::parse_from_rfc3339("2026-09-18T12:00:00Z").unwrap();
    let mut ws = Workspace::load(vec![notes.clone()]).unwrap();
    ws.cache.insert(
        "https://github.com/o/r/pull/42".into(),
        wtf::github::metadata(
            "pull",
            &serde_json::json!({"title": "Cached pull request", "state": "OPEN"}),
            clock.to_utc(),
        )
        .unwrap(),
    );
    ws.save_cache().unwrap();
    let cache_before = std::fs::read(notes.join(".wtf/cache.json")).unwrap();
    let output = success(run(
        root,
        &[
            "render",
            "main.wtf",
            "--root",
            "notes",
            "--now",
            "2026-09-18T12:00:00Z",
        ],
    ));
    assert_eq!(
        output,
        presentation::render_text_in(&RequestContext::new(&ws, clock), &notes.join("main.wtf"))
            .unwrap()
    );
    assert!(output.contains("[total] := price * 2 = $14\r\n"));
    assert!(output.contains("Total [total] $14."));
    assert!(output.lines().next().unwrap().contains("custom 12:00"));
    assert!(output.contains("○ open · just now"));
    assert_eq!(
        std::fs::read_to_string(notes.join("main.wtf")).unwrap(),
        source
    );
    assert_eq!(
        std::fs::read_to_string(notes.join("values.wtf")).unwrap(),
        "[$7]:price\n"
    );
    assert_eq!(
        std::fs::read_to_string(modules.join("headings.wtf")).unwrap(),
        module
    );
    assert_eq!(
        std::fs::read(notes.join(".wtf/cache.json")).unwrap(),
        cache_before
    );
    assert_eq!(
        success(run(
            root,
            &[
                "render",
                notes.join("main.wtf").to_str().unwrap(),
                "--root",
                "notes",
                "--now",
                "2026-09-18T12:00:00Z"
            ]
        )),
        output
    );
}

#[test]
fn render_clock_flags_freeze_time_and_preserve_output_boundaries() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join("clock.wtf"), "clock := now()\nday := today()").unwrap();
    assert_eq!(
        success(run(
            root,
            &["render", "clock.wtf", "--now", "2026-09-18T23:30:00-04:00"]
        )),
        "clock := now() = 2026-09-18 23:30:00 -04:00\nday := today() = 2026-09-18"
    );
    assert_eq!(
        success(run(root, &["render", "clock.wtf", "--on", "2026-09-17"])),
        "clock := now() = 2026-09-17 00:00:00 +00:00\nday := today() = 2026-09-17"
    );
    assert_eq!(
        run(
            root,
            &[
                "render",
                "clock.wtf",
                "--on",
                "2026-09-17",
                "--now",
                "2026-09-18T12:00:00Z"
            ]
        )
        .status
        .code(),
        Some(2)
    );
    assert_eq!(run(root, &["render"]).status.code(), Some(2));
    for source in [
        "",
        "Plain prose.\r\n\r\n",
        "// A comment with 🦀",
        "```\nfoo := missing\n```\n",
    ] {
        std::fs::write(root.join("plain.wtf"), source).unwrap();
        assert_eq!(success(run(root, &["render", "plain.wtf"])), source);
    }
    assert!(!root.join(".wtf").exists());
}

#[test]
fn render_reports_diagnostics_separately_while_preserving_successful_values() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let source = "bad := missing + 1\ngood := 3 + 4\n";
    std::fs::write(root.join("main.wtf"), source).unwrap();
    let output = run(root, &["render", "main.wtf"]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "bad := missing + 1\ngood := 3 + 4 = 7\n"
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("main.wtf:1:"), "{stderr}");
    assert!(stderr.contains("Unknown name 'missing'"), "{stderr}");
    assert!(stderr.contains("error(s) while rendering"), "{stderr}");
    assert_eq!(
        std::fs::read_to_string(root.join("main.wtf")).unwrap(),
        source
    );

    std::fs::write(
        root.join("main.wtf"),
        "## Friday, September 18, 2026\n09:00 Mystery stop\n",
    )
    .unwrap();
    let warning = run(root, &["render", "main.wtf", "--on", "2026-09-18"]);
    assert!(
        warning.status.success(),
        "{}",
        String::from_utf8_lossy(&warning.stderr)
    );
    assert!(String::from_utf8_lossy(&warning.stderr).contains("warning:"));
    assert!(String::from_utf8_lossy(&warning.stdout).contains("09:00 Mystery stop"));
}

#[test]
fn render_rejects_missing_unindexed_and_invalid_module_inputs_without_output() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join(".gitignore"), "ignored.wtf\n").unwrap();
    std::fs::write(root.join("ignored.wtf"), "answer := 42\n").unwrap();
    std::fs::write(root.join("other.txt"), "answer := 42\n").unwrap();
    for path in ["missing.wtf", "ignored.wtf", "other.txt"] {
        let output = run(root, &["render", path]);
        assert_eq!(output.status.code(), Some(1), "{path}");
        assert!(output.stdout.is_empty(), "{path}");
        assert!(!output.stderr.is_empty(), "{path}");
    }
    let notes = root.join("notes");
    std::fs::create_dir(&notes).unwrap();
    std::fs::write(root.join("outside.wtf"), "answer := 42\n").unwrap();
    let output = run(root, &["render", "../outside.wtf", "--root", "notes"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    std::fs::write(root.join("main.wtf"), "answer := 42\n").unwrap();
    std::fs::create_dir_all(root.join(".wtf/modules")).unwrap();
    std::fs::write(root.join(".wtf/modules/bad.wtf"), "module := {api: 99}").unwrap();
    let output = run(root, &["render", "main.wtf"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

#[test]
fn render_defaults_to_standalone_html_and_keeps_text_explicit() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let source = "// <script>alert(1)</script>\nanswer := $3 * 2\nAmount [answer].\n";
    std::fs::write(root.join("main.wtf"), source).unwrap();
    let args = ["render", "main.wtf", "--now", "2026-09-18T12:00:00Z"];
    let output = success(run_format(&root, &args, None));
    let ws = Workspace::load(vec![root.clone()]).unwrap();
    let request = RequestContext::new(
        &ws,
        DateTime::parse_from_rfc3339("2026-09-18T12:00:00Z").unwrap(),
    );
    assert_eq!(
        output,
        wtf::rendering::html_in(&request, &root.join("main.wtf")).unwrap()
    );
    assert_eq!(output, success(run_format(&root, &args, Some("html"))));
    assert!(output.starts_with("<!doctype html>"));
    assert!(output.contains("class=\"t-wtfMoney\""));
    assert!(output.contains("class=\"inlay\""));
    assert!(output.contains("= $6</span>"));
    assert!(output.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!output.contains("<script>"));
    assert!(!success(run(&root, &args)).contains("<!doctype html>"));
    assert_eq!(run_format(&root, &args, Some("pdf")).status.code(), Some(2));
    assert!(!root.join(".wtf").exists());
    assert_eq!(
        std::fs::read_to_string(root.join("main.wtf")).unwrap(),
        source
    );
}
