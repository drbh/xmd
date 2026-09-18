#![cfg(feature = "native")]
use serde_json::{Value, json};
use std::{
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};
fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_wtf"))
        .current_dir(root)
        .env("TZ", "UTC")
        .args(args)
        .output()
        .unwrap()
}
fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
fn json_output(root: &Path, args: &[&str]) -> Value {
    serde_json::from_str(&success(run(root, args))).unwrap()
}

#[test]
fn query_cli_handles_inline_files_stdin_jsonl_and_saved_views_without_writes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let note = "- [ ] Open #work @due(2026-09-16) @estimate(90s)\n- [x] Closed\n";
    std::fs::write(root.join("n.wtf"), note).unwrap();
    std::fs::write(root.join(".gitignore"), "ignored.wtf\n").unwrap();
    std::fs::write(root.join("ignored.wtf"), "- [ ] Invisible\n").unwrap();
    let result = json_output(
        root,
        &[
            "query",
            "@today | select {title, estimate, source}",
            "--on",
            "2026-09-16",
            "--json",
        ],
    );
    assert_eq!(result.as_array().unwrap().len(), 1);
    assert_eq!(
        result[0]["estimate"],
        json!({"type":"duration","seconds":90})
    );
    assert_eq!(result[0]["source"]["line"], 1);
    std::fs::write(root.join("open.wq"), "@tasks | select title").unwrap();
    assert_eq!(
        json_output(root, &["q", "-f", "open.wq", "--json"]),
        json!(["Open #work"])
    );
    let lines = success(run(root, &["query", "tasks | select {title}", "--jsonl"]));
    let lines = lines
        .lines()
        .map(|s| serde_json::from_str::<Value>(s).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        lines,
        json!([{"title":"Open #work"},{"title":"Closed"}])
            .as_array()
            .unwrap()
            .clone()
    );
    let mut child = Command::new(env!("CARGO_BIN_EXE_wtf"))
        .current_dir(root)
        .args(["query", "-f", "-", "--json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"tasks | count")
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&success(child.wait_with_output().unwrap())).unwrap(),
        json!([2])
    );
    assert_eq!(std::fs::read_to_string(root.join("n.wtf")).unwrap(), note);
    assert!(!root.join(".wtf").exists());
}

#[test]
fn diagnostic_exit_status_and_invalid_queries_keep_stdout_machine_readable() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    assert_eq!(
        json_output(root, &["query", "@check", "--fail-on-match", "--json"]),
        json!([])
    );
    std::fs::write(root.join("n.wtf"), "bad := absent + 1\n").unwrap();
    let failed = run(root, &["query", "@check", "--fail-on-match", "--json"]);
    assert_eq!(failed.status.code(), Some(1));
    let rows: Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(rows[0]["severity"], "error");
    assert!(String::from_utf8_lossy(&failed.stderr).contains("matching result"));
    let malformed = run(root, &["query", "tasks | where ("]);
    assert_eq!(malformed.status.code(), Some(1));
    assert!(malformed.stdout.is_empty());
    let incompatible = run(root, &["query", "tasks", "--json", "--jsonl"]);
    assert_eq!(incompatible.status.code(), Some(2));
    let missing = run(root, &["query", "-f", "missing.wq"]);
    assert!(!missing.status.success());
    assert!(missing.stdout.is_empty());
    for old in ["today", "agenda", "tasks", "check", "plan"] {
        assert_eq!(run(root, &[old]).status.code(), Some(2), "{old}");
    }
}

#[test]
fn clock_options_freeze_both_today_and_now_and_root_is_respected() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let notes = root.join("notes");
    std::fs::create_dir(&notes).unwrap();
    std::fs::write(notes.join("time.wtf"), "clock := now()\nday := today()\n").unwrap();
    let pinned = json_output(
        root,
        &[
            "query",
            "values | select value",
            "--root",
            "notes",
            "--now",
            "2026-09-16T23:30:00-04:00",
            "--json",
        ],
    );
    assert_eq!(
        pinned,
        json!([{"type":"datetime","value":"2026-09-16T23:30:00-04:00"},{"type":"date","value":"2026-09-16"}])
    );
    let date = json_output(
        root,
        &[
            "query",
            "values | select value",
            "--root",
            "notes",
            "--on",
            "2026-09-16",
            "--json",
        ],
    );
    assert_eq!(date[0]["value"], "2026-09-16T00:00:00+00:00");
    assert_eq!(
        run(
            root,
            &[
                "query",
                "tasks",
                "--on",
                "2026-09-16",
                "--now",
                "2026-09-16T12:00:00Z"
            ]
        )
        .status
        .code(),
        Some(2)
    );
}
