use chrono::{DateTime, FixedOffset};
use lsp_types::Url;
use serde_json::json;
use std::path::Path;
use wtf::{
    RequestContext,
    commands::{Action, Capabilities, PreparedAction, RowTarget},
    document::Document,
    timers::TimerAction,
    workspace::Workspace,
};

fn path() -> &'static Path {
    Path::new("/workspace/note.wtf")
}
fn uri() -> Url {
    wtf::paths::file_url(path()).unwrap()
}
fn now() -> DateTime<FixedOffset> {
    DateTime::parse_from_rfc3339("2026-09-16T14:00:00-04:00").unwrap()
}
fn workspace(source: &str) -> Workspace {
    Workspace {
        roots: vec!["/workspace".into()],
        documents: [(path().into(), Document::parse(source.into()))].into(),
        cache: Default::default(),
        lookups: Default::default(),
        modules: Default::default(),
    }
}
fn row(ws: &Workspace, row: usize) -> RowTarget {
    RowTarget {
        document: uri(),
        row,
        expected: ws.documents[path()].line(row).into(),
    }
}
fn prepare(ws: &Workspace, action: &Action) -> Result<PreparedAction, String> {
    action.prepare(&RequestContext::new(ws, now()), Capabilities::NATIVE)
}

#[test]
fn every_typed_action_round_trips_through_the_existing_wire_format() {
    let ws = workspace("- [ ] Task\n");
    let target = row(&ws, 0);
    let url = Url::parse("https://example.com/page").unwrap();
    let actions = [
        Action::ToggleTask(target.clone()),
        Action::Timer {
            document: uri(),
            name: "focus".into(),
            action: TimerAction::Start,
        },
        Action::OpenResource {
            target: target.clone(),
            url: url.clone(),
        },
        Action::RefreshResource { target, url },
        Action::Refresh {
            document: Some(uri()),
        },
        Action::Refresh { document: None },
        Action::ShowToday,
    ];
    for action in &actions {
        let command = action.command("Title");
        assert!(Action::COMMANDS.contains(&command.command.as_str()));
        assert_eq!(
            Action::decode(&command.command, command.arguments.as_deref().unwrap()).unwrap(),
            *action
        );
        assert_eq!(command.title, "Title");
    }
    assert_eq!(
        actions[0].command("Task").arguments.unwrap(),
        vec![json!(uri()), json!(0), json!("- [ ] Task")]
    );
    for operation in [
        TimerAction::Start,
        TimerAction::Pause,
        TimerAction::Resume,
        TimerAction::Reset,
    ] {
        assert_eq!(
            operation.as_str().parse::<TimerAction>().unwrap(),
            operation
        );
    }
}

#[test]
fn malformed_commands_are_rejected_before_evaluation_or_host_effects() {
    for (command, args) in [
        ("wtf.task", json!([])),
        ("wtf.task", json!([uri(), 0, "task", "extra"])),
        ("wtf.task", json!([uri(), -1, "task"])),
        ("wtf.task", json!([uri(), "0", "task"])),
        ("wtf.task", json!([uri(), 0, null])),
        ("wtf.task", json!(["https://example.com", 0, "task"])),
        (
            "wtf.task",
            json!(["file:///workspace/note.wtf#L1", 0, "task"]),
        ),
        ("wtf.timer", json!([uri(), "focus", "explode"])),
        ("wtf.timer", json!([uri(), 12, "start"])),
        ("wtf.openResource", json!([uri(), 0, "line", "not a URL"])),
        ("wtf.refresh", json!([uri(), "extra"])),
        ("wtf.today", json!([uri()])),
        ("unknown", json!([])),
    ] {
        assert!(
            Action::decode(command, args.as_array().unwrap()).is_err(),
            "{command}: {args}"
        );
    }
}

#[test]
fn task_preparation_is_undoable_and_rejects_stale_or_blocked_source() {
    let source = "- [ ] Task\n";
    let mut ws = workspace(source);
    let action = Action::ToggleTask(row(&ws, 0));
    let PreparedAction::Edit {
        path: changed,
        edits,
    } = prepare(&ws, &action).unwrap()
    else {
        panic!("expected edit")
    };
    assert_eq!(changed, path());
    assert_eq!(
        wtf::actions::apply_edits(source, &edits).unwrap(),
        "- [x] Task @completed(2026-09-16)\n"
    );
    assert_eq!(ws.documents[path()].text, source);
    ws.documents
        .insert(path().into(), Document::parse("- [ ] Replacement\n".into()));
    assert!(
        prepare(&ws, &action)
            .unwrap_err()
            .contains("Source changed")
    );
    let ws = workspace("- [ ] Waiting @after(prerequisite)\n- [ ] First :prerequisite\n");
    assert!(
        prepare(&ws, &Action::ToggleTask(row(&ws, 0)))
            .unwrap_err()
            .contains("Blocked by")
    );
    let ws = workspace("plain text\n");
    assert!(
        prepare(&ws, &Action::ToggleTask(row(&ws, 0)))
            .unwrap_err()
            .contains("No task")
    );
    let action = Action::ToggleTask(RowTarget {
        document: uri(),
        row: usize::MAX,
        expected: String::new(),
    });
    assert!(
        prepare(&ws, &action)
            .unwrap_err()
            .contains("Source changed")
    );
}

#[test]
fn timer_preparation_uses_current_state_clock_and_cross_note_origin() {
    let mut ws = workspace("[alias] := import(\"./timer.wtf\").focus\n");
    let timer_path = Path::new("/workspace/timer.wtf");
    let source = "[focus] := stopwatch()\n";
    ws.documents
        .insert(timer_path.into(), Document::parse(source.into()));
    let action = Action::Timer {
        document: uri(),
        name: "alias".into(),
        action: TimerAction::Start,
    };
    let PreparedAction::Edit {
        path: changed,
        edits,
    } = prepare(&ws, &action).unwrap()
    else {
        panic!("expected edit")
    };
    assert_eq!(changed, timer_path);
    let running = wtf::actions::apply_edits(source, &edits).unwrap();
    assert!(running.contains("stopwatch(0s, 2026-09-16T14:00:00-04:00)"));
    ws.documents
        .insert(timer_path.into(), Document::parse(running.clone()));
    assert!(
        prepare(&ws, &action)
            .unwrap_err()
            .contains("Cannot start a running timer")
    );
    let pause = Action::Timer {
        document: uri(),
        name: "alias".into(),
        action: TimerAction::Pause,
    };
    let later = RequestContext::new(&ws, now() + chrono::Duration::seconds(75));
    let PreparedAction::Edit { edits, .. } = pause.prepare(&later, Capabilities::BROWSER).unwrap()
    else {
        panic!("expected edit")
    };
    assert!(
        wtf::actions::apply_edits(&running, &edits)
            .unwrap()
            .contains("stopwatch(75s)")
    );
}

#[test]
fn resource_preparation_rechecks_aliases_and_refresh_support() {
    let mut ws = workspace("See [site].\nsite := import(\"./links.wtf\").site\n");
    let other = Path::new("/workspace/links.wtf");
    let url = Url::parse("https://example.com/old").unwrap();
    ws.documents
        .insert(other.into(), Document::parse(format!("[{url}]:site\n")));
    let target = row(&ws, 0);
    let action = Action::OpenResource {
        target: target.clone(),
        url: url.clone(),
    };
    assert!(
        matches!(prepare(&ws,&action).unwrap(),PreparedAction::Open {url:opened} if opened==url)
    );
    assert!(
        prepare(
            &ws,
            &Action::RefreshResource {
                target: target.clone(),
                url
            }
        )
        .unwrap_err()
        .contains("does not support refresh")
    );
    let github = Url::parse("https://github.com/acme/app/pull/42").unwrap();
    ws.documents
        .insert(other.into(), Document::parse(format!("[{github}]:site\n")));
    assert!(
        prepare(&ws, &action)
            .unwrap_err()
            .contains("Resource changed")
    );
    let refresh = Action::RefreshResource {
        target,
        url: github,
    };
    assert!(matches!(
        prepare(&ws, &refresh).unwrap(),
        PreparedAction::RefreshResource { .. }
    ));
    assert!(
        refresh
            .prepare(&RequestContext::new(&ws, now()), Capabilities::BROWSER)
            .unwrap_err()
            .contains("not available")
    );
    ws.documents.remove(path());
    assert!(prepare(&ws, &action).unwrap_err().contains("no longer"));
}

#[test]
fn capabilities_control_both_available_controls_and_execution() {
    let ws = workspace(
        "[focus] := stopwatch()\n- [ ] Task\nhttps://github.com/acme/app/pull/42\n[price] := quote(ACME)\n",
    );
    let request = RequestContext::new(&ws, now());
    let native = request.code_lenses(path(), Capabilities::NATIVE);
    let browser = request.code_lenses(path(), Capabilities::BROWSER);
    assert!(
        native
            .iter()
            .any(|l| l.command.as_ref().unwrap().command == "wtf.refresh")
    );
    assert!(
        native
            .iter()
            .any(|l| l.command.as_ref().unwrap().command == "wtf.refreshResource")
    );
    for lens in browser {
        let command = lens.command.unwrap();
        let action = Action::decode(
            &command.command,
            command.arguments.as_deref().unwrap_or(&[]),
        )
        .unwrap();
        assert!(Capabilities::BROWSER.supports(&action));
        assert!(matches!(
            action.prepare(&request, Capabilities::BROWSER).unwrap(),
            PreparedAction::Edit { .. } | PreparedAction::Open { .. }
        ));
    }
    assert!(
        Action::Refresh { document: None }
            .prepare(&request, Capabilities::BROWSER)
            .is_err()
    );
    assert!(
        Action::ShowToday
            .prepare(&request, Capabilities::BROWSER)
            .is_err()
    );
}
