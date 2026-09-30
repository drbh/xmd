//! The task provider: completing, reopening and advancing tasks. It proposes
//! the toggle on every task row it would succeed on, and prepares
//! `toggle_task` at execution time, against that moment's clock. Completing
//! a recurring task moves it to its next occurrence instead of checking it.
use crate::Request;
use crate::commands::{
    Action, ActionProvider, Ask, Capabilities, CommandId, NOT_MINE, Prepared, PreparedAction,
    Proposal, RowTarget, TaskToggle,
};
use chrono::NaiveDate;
use lang::common::Span;
use lang::eval::engine::{Engine, next_occurrence};
use lang::model::{Document, TaskState, end_position};
use lang::stdlib;
use lang::syntax::AttributeKey;
use lsp_types::{Range, TextEdit};
use std::{collections::BTreeMap, path::Path};

// The attributes completing a task reads and writes, spelled by the table.
const EVERY: &str = AttributeKey::Every.as_str();
const DUE: &str = AttributeKey::Due.as_str();
const REPEAT_FROM: &str = AttributeKey::RepeatFrom.as_str();
const COMPLETED: &str = AttributeKey::Completed.as_str();

pub(crate) struct Tasks;
impl ActionProvider for Tasks {
    fn kinds(&self) -> &'static [CommandId] {
        &[CommandId::ToggleTask]
    }
    /// The toggle, when the host takes it as a command; as an action it is a
    /// code action of its own.
    fn propose(&self, request: &Request<'_>, path: &Path, ask: Ask) -> Vec<Proposal> {
        let Some(doc) = request.workspace().documents().get(path) else {
            return vec![];
        };
        if ask.toggle != TaskToggle::Command {
            return vec![];
        }
        let Ok(uri) = lang::common::file_url(path) else {
            return vec![];
        };
        // A title depends only on what kind of toggle it is, so each kind is
        // worded once per note.
        let mut titles = BTreeMap::new();
        let mut seen = None;
        let mut proposals = vec![];
        for (index, task) in doc.tasks.iter().enumerate() {
            // The first task on a row is the one its toggle acts on.
            if !ask.rows.has(task.line) || seen == Some(task.line) {
                continue;
            }
            seen = Some(task.line);
            if toggle(request, path, index).is_ok() {
                let mut engine = request.engine();
                proposals.push(Proposal {
                    line: task.line,
                    title: titles
                        .entry(kind(&mut engine, path, index))
                        .or_insert_with_key(|&(recurring, done)| {
                            worded(&mut engine, recurring, done)
                        })
                        .clone(),
                    action: Action::ToggleTask(RowTarget::at(&uri, doc, task.line)),
                });
            }
        }
        proposals
    }
    fn prepare(
        &self,
        request: &Request<'_>,
        action: &Action,
        _: Capabilities,
    ) -> Result<Prepared, String> {
        let Action::ToggleTask(target) = action else {
            return Err(NOT_MINE.into());
        };
        let (path, doc) = target.validate(request)?;
        let index = doc
            .tasks
            .iter()
            .position(|t| t.line == target.row)
            .ok_or("No task at this line")?;
        let edits = toggle(request, &path, index)?;
        Ok(PreparedAction::Edit { path, edits }.into())
    }
}

/// The shared title for the task toggle lens and code action.
pub(crate) fn title(engine: &mut Engine<'_>, path: &Path, index: usize) -> String {
    let (recurring, done) = kind(engine, path, index);
    worded(engine, recurring, done)
}
/// Which toggle a task has: whether it recurs, and whether it is done.
fn kind(engine: &mut Engine<'_>, path: &Path, index: usize) -> (bool, bool) {
    let recurring = engine.workspace().documents()[path].tasks[index]
        .attributes
        .contains_key(EVERY);
    (recurring, engine.task_done(path, index))
}
fn worded(engine: &mut Engine<'_>, recurring: bool, done: bool) -> String {
    stdlib::shown(stdlib::task::toggle(engine, recurring, done))
}

/// The edits that complete, reopen or advance the `index`th task of the note
/// at `path` today, or why it cannot be toggled.
pub(crate) fn toggle(
    request: &Request<'_>,
    path: &Path,
    index: usize,
) -> Result<Vec<TextEdit>, String> {
    let workspace = request.workspace();
    let today = request.today();

    let doc = &workspace.documents()[path];
    let task = doc.tasks.get(index).ok_or("No task at this line")?;
    let mut engine = request.engine();
    let done = engine.task_done(path, index);
    let mut indices = vec![index];
    for i in index + 1..doc.tasks.len() {
        if doc.tasks[i].indent <= task.indent {
            break;
        }
        indices.push(i);
    }
    if !done {
        for i in &indices {
            let blocked = engine.blocked(path, *i)?;
            if !blocked.is_empty() {
                return Err(format!("Blocked by {}", blocked.join(", ")));
            }
        }
    }
    if indices.len() > 1
        && indices
            .iter()
            .any(|i| doc.tasks[*i].attributes.contains_key(EVERY))
    {
        return Err(
            "Complete recurring tasks individually; recurring parent tasks are unsupported".into(),
        );
    }
    match task.attributes.get(EVERY) {
        Some(recurrence) => recur(&mut engine, path, index, &recurrence.value, today),
        None => Ok(check(doc, &indices, done, today)),
    }
}
/// Completing a recurring task moves it to its next occurrence and records the
/// completion as history, leaving its checkbox alone.
fn recur(
    engine: &mut Engine<'_>,
    path: &Path,
    index: usize,
    recurrence: &str,
    today: NaiveDate,
) -> Result<Vec<TextEdit>, String> {
    let doc = &engine.workspace().documents()[path];
    let task = &doc.tasks[index];
    if task.state == TaskState::Done {
        return Err("A recurring task should remain unchecked; remove [x] to resume it".into());
    }
    let due = task
        .attributes
        .get(DUE)
        .map(|a| engine.when(path, &a.value).and_then(|v| engine.date(&v)))
        .transpose()?
        .unwrap_or(today);
    let anchor = task
        .attributes
        .get(REPEAT_FROM)
        .map(|a| {
            lang::syntax::stamp(&a.value).ok_or_else(|| {
                let example = AttributeKey::RepeatFrom.example();
                format!("@{REPEAT_FROM} requires a calendar date, e.g. @{REPEAT_FROM}({example})")
            })
        })
        .transpose()?
        .unwrap_or(due);
    let next = next_occurrence(recurrence, anchor, today.max(due))?;
    let mut edits = Vec::new();
    if let Some(attr) = task.attributes.get(DUE) {
        edits.push(TextEdit::new(attr.value_span.range(doc), next.to_string()));
    }
    let mut suffix = String::new();
    if !task.attributes.contains_key(DUE) {
        suffix.push_str(&format!(" @{DUE}({next})"));
    }
    if !task.attributes.contains_key(REPEAT_FROM) {
        suffix.push_str(&format!(" @{REPEAT_FROM}({anchor})"));
    }
    if !suffix.is_empty() {
        edits.push(TextEdit::new(
            Range::new(doc.line_end(task.line), doc.line_end(task.line)),
            suffix,
        ));
    }
    let history = serde_json::json!({"task":task.named.as_ref().map(|n|n.name.as_str()),"title":task.title,"completed":today,"due":due,"next":next});
    let end = end_position(&doc.text);
    let newline = if doc.text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let prefix = if doc.text.ends_with('\n') {
        ""
    } else {
        newline
    };
    edits.push(TextEdit::new(
        Range::new(end, end),
        format!(
            "{prefix}<!-- xmd-history {} -->{newline}",
            history.to_string().replace("-->", "--\\u003e")
        ),
    ));
    Ok(edits)
}
/// Checking or unchecking a plain task and its subtasks, stamping or clearing
/// when each was completed.
fn check(doc: &Document, indices: &[usize], done: bool, today: NaiveDate) -> Vec<TextEdit> {
    let mut edits = Vec::new();
    for &i in indices {
        let task = &doc.tasks[i];
        let span = Span::new(task.line, task.checkbox.start + 1, task.checkbox.start + 2);
        edits.push(TextEdit::new(
            span.range(doc),
            if done { " ".into() } else { "x".into() },
        ));
        if let Some(attr) = task.attributes.get(COMPLETED) {
            edits.push(TextEdit::new(
                attr.span.range(doc),
                if done {
                    String::new()
                } else {
                    format!("@{COMPLETED}({today})")
                },
            ));
        } else if !done {
            edits.push(TextEdit::new(
                Range::new(doc.line_end(task.line), doc.line_end(task.line)),
                format!(" @{COMPLETED}({today})"),
            ));
        }
    }
    edits
}

#[cfg(test)]
mod tests {
    use super::*;
    use lang::eval::Workspace;
    use lang::model::apply_edits;

    /// Completing a task writes attributes; each one it writes must be in the
    /// attribute table, so the parser knows it and highlighting, diagnostics
    /// and completion describe it.
    #[test]
    fn completing_a_task_writes_only_attributes_in_the_table() {
        let text = "- [ ] Plain :plain\n  - [ ] Child\n- [ ] Rent :rent @every(month)\n";
        let path = Path::new("/notes/tasks.x.md");
        let mut workspace = Workspace::new(vec!["/notes".into()]);
        workspace.insert_document(path.into(), Document::parse(text.into()));
        let now = chrono::DateTime::parse_from_rfc3339("2026-09-18T09:00:00-04:00").unwrap();
        let request = crate::Request::new(&workspace, now);
        let mut written = std::collections::BTreeSet::new();
        for index in [0, 2] {
            let edits = toggle(&request, path, index).unwrap();
            let after = Document::parse(apply_edits(text, &edits).unwrap());
            assert!(
                after.problems.is_empty(),
                "completing task {index} left problems: {:?}",
                after
                    .problems
                    .iter()
                    .map(|p| &p.message)
                    .collect::<Vec<_>>()
            );
            let before = &workspace.documents()[path].tasks;
            for (task, old) in after.tasks.iter().zip(before) {
                for key in task.attributes.keys() {
                    assert!(
                        key.parse::<AttributeKey>().is_ok(),
                        "completing a task wrote @{key}, which is not in the attribute table"
                    );
                    if !old.attributes.contains_key(key) {
                        written.insert(key.clone());
                    }
                }
            }
        }
        // Both actions ran: the plain task was stamped, the recurring one moved.
        assert_eq!(
            written.into_iter().collect::<Vec<_>>(),
            [COMPLETED, DUE, REPEAT_FROM]
        );
    }
}
