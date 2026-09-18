use chrono::{DateTime, Duration, FixedOffset};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use tower_lsp::lsp_types::{Position, Range};
use wtf::{
    actions,
    document::Document,
    editor,
    engine::{Engine, Value},
    timers,
    workspace::Workspace,
};

fn at(seconds: i64) -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-09-16T14:00:00-04:00").unwrap() + Duration::seconds(seconds)
}
fn notes(text: &str) -> Workspace {
    Workspace {
        roots: vec![PathBuf::from("/notes")],
        documents: [(
            PathBuf::from("/notes/timers.wtf"),
            Document::parse(text.into()),
        )]
        .into(),
        cache: BTreeMap::new(),
        lookups: Default::default(),
    }
}
fn path() -> &'static Path {
    Path::new("/notes/timers.wtf")
}
fn eval(ws: &Workspace, expr: &str, seconds: i64) -> Value {
    Engine::at(ws, at(seconds)).eval(path(), expr).unwrap()
}
fn change(ws: &mut Workspace, name: &str, action: &str, seconds: i64) {
    let (origin, edit) = timers::edit(ws, path(), name, action, at(seconds)).unwrap();
    let text = actions::apply_edits(&ws.documents[&origin.path].text, &[edit]).unwrap();
    ws.documents.insert(origin.path, Document::parse(text));
}

#[test]
fn declarations_are_idle_and_reading_never_starts_or_changes_them() {
    let text = "[focus] := countdown(25m)\n[watch] := stopwatch()\nTime left: [focus.remaining].\n";
    let ws = notes(text);
    for t in [0, 10, 86400] {
        assert_eq!(eval(&ws, "watch.elapsed", t), Value::Duration(0));
        assert_eq!(eval(&ws, "focus.remaining", t), Value::Duration(1500));
        assert_eq!(eval(&ws, "focus.running", t), Value::Bool(false));
        assert_eq!(eval(&ws, "focus.done", t), Value::Bool(false));
        let mut engine = Engine::at(&ws, at(t));
        engine.eval(path(), "focus").unwrap();
        assert!(!engine.time_dependent);
    }
    assert_eq!(ws.documents[path()].text, text);
    assert!(editor::problems(&ws, path(), at(0).date_naive()).is_empty());
}

#[test]
fn stopwatch_start_pause_resume_reset_and_reload() {
    let mut ws = notes("🦀 [watch] := stopwatch()\r\nKeep this prose.\r\n");
    assert!(timers::edit(&ws, path(), "watch", "pause", at(0)).is_err());
    change(&mut ws, "watch", "start", 0);
    assert!(
        ws.documents[path()]
            .text
            .contains("stopwatch(0s, 2026-09-16T14:00:00-04:00)")
    );
    assert_eq!(eval(&ws, "watch.elapsed", 73), Value::Duration(73));
    change(&mut ws, "watch", "pause", 73);
    assert!(ws.documents[path()].text.contains("stopwatch(73s)"));
    // A fresh parse/evaluator is also what a restarted language server sees.
    ws = notes(&ws.documents[path()].text);
    assert_eq!(eval(&ws, "watch.elapsed", 999), Value::Duration(73));
    assert_eq!(eval(&ws, "watch.state", 999), Value::Text("paused".into()));
    assert!(timers::edit(&ws, path(), "watch", "start", at(999)).is_err());
    change(&mut ws, "watch", "resume", 1000);
    assert_eq!(eval(&ws, "watch.elapsed", 1009), Value::Duration(82));
    ws = notes(&ws.documents[path()].text);
    assert_eq!(eval(&ws, "watch.elapsed", 4600), Value::Duration(3673));
    change(&mut ws, "watch", "reset", 4600);
    assert_eq!(
        ws.documents[path()].text,
        "🦀 [watch] := stopwatch()\r\nKeep this prose.\r\n"
    );
}

#[test]
fn countdown_clamps_at_zero_and_preserves_duration_expression() {
    let mut ws = notes("[session] := 2m\n[focus] := countdown(session + (30s * 2))");
    change(&mut ws, "focus", "start", 0);
    assert_eq!(eval(&ws, "focus.remaining", 63), Value::Duration(117));
    change(&mut ws, "focus", "pause", 63);
    assert_eq!(eval(&ws, "focus.remaining", 999), Value::Duration(117));
    change(&mut ws, "focus", "resume", 1000);
    assert_eq!(eval(&ws, "focus.remaining", 1116), Value::Duration(1));
    assert_eq!(eval(&ws, "focus.remaining", 1117), Value::Duration(0));
    assert_eq!(eval(&ws, "focus.elapsed", 9000), Value::Duration(180));
    assert_eq!(eval(&ws, "focus.done", 9000), Value::Bool(true));
    assert_eq!(eval(&ws, "focus.running", 9000), Value::Bool(false));
    let mut engine = Engine::at(&ws, at(9000));
    assert!(
        engine
            .eval(path(), "focus")
            .unwrap()
            .display()
            .contains("00:00 remaining · ✓ done")
    );
    assert!(!engine.time_dependent);
    assert!(timers::edit(&ws, path(), "focus", "resume", at(9000)).is_err());
    change(&mut ws, "focus", "reset", 9000);
    assert_eq!(
        ws.documents[path()].text,
        "[session] := 2m\n[focus] := countdown(session + (30s * 2))"
    );
}

#[test]
fn seconds_work_through_dates_effort_cli_and_comparisons() {
    let ws = notes(
        "# Tasks :tasks\n- [ ] First @estimate(90s)\n- [ ] Second @estimate(30m)\n[watch] := stopwatch(31s)\n",
    );
    for (expr, expected) in [
        ("30s + 1m", Value::Duration(90)),
        ("1m / 2", Value::Duration(30)),
        ("0.5m", Value::Duration(30)),
        ("watch.elapsed > 30s", Value::Bool(true)),
        ("2026-09-16T14:00:00-04:00 + 45s", Value::DateTime(at(45))),
        (
            "2026-09-16T14:00:45-04:00 - 2026-09-16T14:00:00-04:00",
            Value::Duration(45),
        ),
        ("2026-09-17 - 2026-09-16", Value::Duration(86400)),
        ("effort(tasks)", Value::Duration(1890)),
    ] {
        assert_eq!(eval(&ws, expr, 0), expected, "{expr}");
    }
    assert!(
        Engine::at(&ws, at(0))
            .eval(path(), "2026-09-16 + 1s")
            .is_err()
    );
    let entries = wtf::query::execute(
        &ws,
        &wtf::query::Query::parse("tasks | where leaf | select estimate").unwrap(),
        &wtf::query::QueryContext::new(at(0)),
    )
    .unwrap();
    assert_eq!(
        entries.rows[0].json(),
        serde_json::json!({"type":"duration","seconds":90})
    );
    assert_eq!(
        entries.rows[1].json(),
        serde_json::json!({"type":"duration","seconds":1800})
    );
    assert_eq!(
        wtf::engine::next_occurrence("2w", at(0).date_naive(), at(0).date_naive()).unwrap(),
        at(14 * 86400).date_naive()
    );
}

#[test]
fn timers_reject_invalid_arguments_and_unsupported_properties() {
    let ws = notes("");
    for expr in [
        "countdown()",
        "countdown(0s)",
        "countdown(-1m)",
        "countdown(25)",
        "stopwatch(-1s)",
        "stopwatch(0s, 2026-09-16)",
        "stopwatch(0s, true)",
        "stopwatch(0s, now(), 1s)",
        "countdown(1m, -1s)",
        "countdown(1m, 0s, now(), 1s)",
        "stopwatch().remaining",
        "countdown(1m).bogus",
        "countdown(0.1s)",
        "stopwatch(9223372036854775808s)",
    ] {
        assert!(Engine::at(&ws, at(0)).eval(path(), expr).is_err(), "{expr}");
    }
    let ws = notes("[watch] := stopwatch()\n- [ ] Bad @timer(1m)\n[watch.bogus]\n");
    let issues = editor::problems(&ws, path(), at(0).date_naive());
    assert!(issues.iter().any(|p| p.message.contains("@timer requires")));
    assert!(
        issues
            .iter()
            .any(|p| p.message.contains("Unknown timer property"))
    );
}

#[test]
fn cross_file_alias_controls_edit_original_and_reference_spans_exclude_properties() {
    let mut ws =
        notes("[alias] := focus\nRemaining [alias.remaining].\n- [ ] Work @timer(alias)\n");
    let origin = PathBuf::from("/notes/shared.wtf");
    ws.documents.insert(
        origin.clone(),
        Document::parse("[focus] := countdown(25m)\n".into()),
    );
    change(&mut ws, "alias", "start", 0);
    assert!(ws.documents[&origin].text.contains("countdown(25m, 0s,"));
    assert_eq!(eval(&ws, "alias.remaining", 60), Value::Duration(1440));
    assert!(editor::problems(&ws, path(), at(0).date_naive()).is_empty());
    let reference = ws.documents[path()]
        .references
        .iter()
        .find(|r| r.bracket)
        .unwrap();
    assert_eq!(reference.name, "alias");
    assert_eq!(reference.expression(), "alias.remaining");
    let line = ws.documents[path()].line(reference.span.line);
    assert_eq!(&line[reference.span.start..reference.span.end], "alias");
    let hints = editor::hints_at(
        &ws,
        path(),
        at(60),
        Range::new(Position::new(0, 0), Position::new(99, 0)),
    );
    let text = serde_json::to_string(&hints).unwrap();
    assert!(text.contains("24:00 remaining"));
    assert!(text.contains("24m"));
    assert!(hints.iter().any(|h| h.position == Position::new(1, 27)));
}

#[test]
fn injected_clock_is_consistent_and_backward_time_never_goes_negative() {
    let ws = notes("[watch] := stopwatch(10s, 2026-09-16T14:00:00-04:00)\n[alias] := watch\n");
    assert_eq!(eval(&ws, "watch.elapsed", -10), Value::Duration(10));
    assert_eq!(
        eval(&ws, "watch.elapsed - alias.elapsed", 120),
        Value::Duration(0)
    );
    assert_eq!(eval(&ws, "now() - now()", 120), Value::Duration(0));
    assert_eq!(eval(&ws, "now()", 120), Value::DateTime(at(120)));
    assert_eq!(
        eval(&ws, "now()", 45).display(),
        "2026-09-16 14:00:45 -04:00"
    );
    assert_eq!(Value::Duration(1122).display(), "18m 42s");
    let mut engine = Engine::at(&ws, at(0));
    engine.eval(path(), "alias.elapsed").unwrap();
    assert!(engine.time_dependent);
    assert!(
        eval(&ws, "watch", 3599)
            .display()
            .contains("01:00:09 elapsed")
    );
}

#[test]
fn grouped_declarations_whitespace_and_timer_dependency_cycles() {
    let mut ws =
        notes("[limit] := 30s\n[watch] := ((countdown(limit)))\n🦀 Left [ watch.remaining ]!");
    change(&mut ws, "watch", "start", 0);
    assert!(ws.documents[path()].text.contains("countdown(limit, 0s,"));
    assert_eq!(eval(&ws, "watch.remaining", 10), Value::Duration(20));
    let hints = editor::hints_at(
        &ws,
        path(),
        at(10),
        Range::new(Position::new(0, 0), Position::new(99, 0)),
    );
    assert!(hints.iter().any(|h| h.position == Position::new(2, 27)));
    let ws = notes("[a] := countdown(b.remaining)\n[b] := countdown(a.remaining)\n");
    assert!(
        Engine::at(&ws, at(0))
            .named(path(), "a")
            .unwrap_err()
            .contains("cycle")
    );
}
