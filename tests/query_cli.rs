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
fn run_stdin(root: &Path, args: &[&str], source: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_wtf"))
        .current_dir(root)
        .env("TZ", "UTC")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn query_cli_handles_expressions_stdin_and_jsonl_without_writes() {
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
            "--workspace",
            "import(\"agenda\").between(entries, today(), today()) | select {title, estimate, source}",
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
    assert_eq!(
        json_output(
            root,
            &[
                "q",
                "--workspace",
                "map(filter(tasks, fn(t) => t.leaf && !t.done), fn(t) => t.title)",
                "--json"
            ]
        ),
        json!(["Open #work"])
    );
    let lines = success(run(
        root,
        &["query", "--workspace", "tasks | select {title}", "--jsonl"],
    ));
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
    let output = run_stdin(
        root,
        &["query", "--workspace", "-", "--json"],
        "tasks | count",
    );
    assert_eq!(
        serde_json::from_str::<Value>(&success(output)).unwrap(),
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
        json_output(
            root,
            &[
                "query",
                "--workspace",
                "diagnostics | where severity == \"error\"",
                "--fail-on-match",
                "--json"
            ]
        ),
        json!([])
    );
    std::fs::write(root.join("n.wtf"), "bad := absent + 1\n").unwrap();
    let failed = run(
        root,
        &[
            "query",
            "--workspace",
            "diagnostics | where severity == \"error\"",
            "--fail-on-match",
            "--json",
        ],
    );
    assert_eq!(failed.status.code(), Some(1));
    let rows: Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(rows[0]["severity"], "error");
    assert!(String::from_utf8_lossy(&failed.stderr).contains("matching result"));
    let malformed = run(root, &["query", "--workspace", "tasks | where ("]);
    assert_eq!(malformed.status.code(), Some(1));
    assert!(malformed.stdout.is_empty());
    let incompatible = run(
        root,
        &["query", "--workspace", "tasks", "--json", "--jsonl"],
    );
    assert_eq!(incompatible.status.code(), Some(2));
    for old in [
        "today", "agenda", "tasks", "check", "plan", "capture", "complete",
    ] {
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
            "--workspace",
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
            "--workspace",
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
                "--workspace",
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

#[test]
fn file_queries_and_inspection_shortcuts_share_outputs_and_never_write() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("notes")).unwrap();
    let source = "a := import(\"./two.wtf\").rate * 2\n- [ ] Local\n";
    std::fs::write(root.join("notes/one.wtf"), source).unwrap();
    std::fs::write(root.join("notes/two.wtf"), "3:rate\n- [ ] Other\n").unwrap();
    let args = [
        "query",
        "one.wtf",
        "map(tasks, fn(t) => t.title)",
        "--root",
        "notes",
        "--json",
    ];
    assert_eq!(json_output(root, &args), json!(["Local"]));
    let ast = json_output(root, &["ast", "one.wtf", "--root", "notes"]);
    assert_eq!(
        ast,
        json_output(
            root,
            &["query", "one.wtf", "ast", "--root", "notes", "--json"]
        )
    );
    assert_eq!(ast[0]["text"], source);
    let graph = json_output(root, &["graph", "one.wtf", "--root", "notes"]);
    assert_eq!(
        graph,
        json_output(
            root,
            &["query", "one.wtf", "graph", "--root", "notes", "--json"]
        )
    );
    assert_eq!(
        json_output(
            root,
            &[
                "graph",
                "one.wtf",
                "--root",
                "notes",
                "--query",
                "graph.nodes | where external | select name"
            ]
        ),
        json!(["rate"])
    );
    assert_eq!(
        json_output(
            root,
            &[
                "ast",
                "one.wtf",
                "--root",
                "notes",
                "--query",
                "map(filter(ast, fn(n) => n.kind == \"definition\"), fn(n) => n.name)"
            ]
        ),
        json!(["a"])
    );
    assert_eq!(
        json_output(
            root,
            &[
                "query",
                root.join("notes/one.wtf").to_str().unwrap(),
                "length(tasks)",
                "--json"
            ]
        ),
        json!([1])
    );
    std::fs::write(root.join("notes/.gitignore"), "ignored.wtf\n").unwrap();
    std::fs::write(root.join("notes/ignored.wtf"), "1:secret\n").unwrap();
    std::fs::write(root.join("outside.wtf"), "- [ ] Outside\n").unwrap();
    let output = run(root, &["ast", "missing.wtf", "--root", "notes"]);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    for file in ["ignored.wtf", "../outside.wtf"] {
        assert!(
            run(root, &["ast", file, "--root", "notes"])
                .status
                .success()
        );
    }
    for command in ["capture", "complete"] {
        let output = run(root, &[command, "one.wtf"]);
        assert_eq!(output.status.code(), Some(2));
    }
    assert_eq!(
        std::fs::read_to_string(root.join("notes/one.wtf")).unwrap(),
        source
    );
    assert!(!root.join("notes/.wtf").exists());
}

#[test]
fn expressions_and_stdin_preserve_positional_and_workspace_scopes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::write(root.join("one note.wtf"), "- [ ] Local\n").unwrap();
    std::fs::write(root.join("other.wtf"), "- [ ] Other\n").unwrap();
    assert_eq!(
        json_output(root, &["q", "one note.wtf", "length(tasks)", "--json"]),
        json!([1])
    );
    assert_eq!(
        json_output(root, &["q", "--workspace", "length(tasks)", "--json"]),
        json!([2])
    );
    assert_eq!(
        json_output(root, &["query", "length(tasks)", "--workspace", "--json"]),
        json!([2])
    );
    for (scope, expected) in [("one note.wtf", json!([1])), ("--workspace", json!([2]))] {
        let output = run_stdin(
            root,
            &["query", scope, "-", "--json"],
            "// Count unfinished tasks.\nlength(\n  filter(tasks, fn(t) => !t.done)\n)\n",
        );
        assert_eq!(
            serde_json::from_str::<Value>(&success(output)).unwrap(),
            expected
        );
    }
    // An expression that resembles a filename stays an expression with --workspace.
    assert_eq!(
        json_output(
            root,
            &["query", "--workspace", "\"one note.wtf\"", "--json"]
        ),
        json!(["one note.wtf"])
    );
}

#[test]
fn ambiguous_or_incomplete_query_arguments_are_rejected_before_reading_input() {
    let tmp = tempfile::tempdir().unwrap();
    for args in [
        vec!["query"],
        vec!["query", "tasks"],
        vec!["query", "note.wtf"],
        vec!["query", "--workspace"],
        vec!["query", "-f", "expression.txt"],
        vec!["query", "note.wtf", "tasks", "--workspace"],
        vec!["query", "note.wtf", "tasks", "-f", "expression.txt"],
        vec!["query", "--workspace", "tasks", "--file", "expression.txt"],
        vec!["query", "--workspace", "tasks", "-f", "-"],
        vec!["query", "tasks", "--in", "note.wtf"],
        vec!["query", "note.wtf", "tasks", "extra"],
    ] {
        let output = run(tmp.path(), &args);
        assert_eq!(output.status.code(), Some(2), "{args:?}");
        assert!(output.stdout.is_empty(), "{args:?}");
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("No such file"),
            "{args:?}"
        );
    }
    let help = success(run(tmp.path(), &["query", "--help"]));
    for usage in ["<FILE> <QUERY>", "--workspace <QUERY>", "- as QUERY"] {
        assert!(help.contains(usage), "{help}");
    }
    assert!(!help.contains("QUERY_FILE"), "{help}");
}

#[test]
fn stdin_queries_keep_the_shared_source_size_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let expression = format!("1 //{}", "x".repeat(65_532));
    let args = ["query", "--workspace", "-", "--json"];
    assert_eq!(
        serde_json::from_str::<Value>(&success(run_stdin(tmp.path(), &args, &expression))).unwrap(),
        json!([1.0])
    );
    let output = run_stdin(tmp.path(), &args, &(expression + "x"));
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("64 KiB"));
}

#[test]
fn explicit_query_imports_load_hidden_notes_without_reading_unrelated_files() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join(".hidden")).unwrap();
    std::fs::write(root.join("main.wtf"), "- [ ] Local\n").unwrap();
    std::fs::write(root.join(".hidden/values.wtf"), "price := $7\n").unwrap();
    std::fs::write(root.join("unrelated.wtf"), [0xff]).unwrap();
    assert_eq!(
        json_output(
            root,
            &[
                "query",
                "main.wtf",
                "import(\"./.hidden/values.wtf\").price",
                "--json"
            ]
        ),
        json!([{"type":"money", "amount":7.0, "currency":"USD"}])
    );
    assert_eq!(
        json_output(
            root,
            &[
                "query",
                "main.wtf",
                "tasks | select import(\"./.hidden/values.wtf\").price",
                "--json"
            ]
        ),
        json!([{"type":"money", "amount":7.0, "currency":"USD"}])
    );
    let missing = run(root, &["query", "main.wtf", "price", "--json"]);
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("Unknown name 'price'"));
    assert!(missing.stdout.is_empty());
}

#[test]
fn query_imports_follow_row_context_and_preserve_lazy_branches() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    for (folder, amount) in [("one", 1), ("two", 2)] {
        std::fs::create_dir_all(root.join(folder).join(".hidden")).unwrap();
        std::fs::write(root.join(folder).join("main.wtf"), "- [ ] Task\n").unwrap();
        std::fs::write(
            root.join(folder).join(".hidden/value.wtf"),
            format!("amount := {amount}\n"),
        )
        .unwrap();
    }
    assert_eq!(
        json_output(
            root,
            &[
                "query",
                "--workspace",
                "tasks | select import(\"./.hidden/value.wtf\").amount",
                "--json"
            ]
        ),
        json!([1.0, 2.0])
    );
    assert_eq!(
        json_output(
            root,
            &[
                "query",
                "one/main.wtf",
                "if(false, import(\"./absent.wtf\").amount, 42)",
                "--json"
            ]
        ),
        json!([42.0])
    );
    assert_eq!(
        json_output(
            root,
            &[
                "query",
                "one/main.wtf",
                "tasks | count | select import(\"./.hidden/value.wtf\").amount",
                "--json"
            ]
        ),
        json!([1.0])
    );
}
