use crate::commands::{Action, Capabilities};
use crate::{providers, rows};
use analysis::{CodeActionItem, refactors};
use chrono::NaiveDate;
use lang::common::Span;
use lang::eval::engine::{Engine, next_occurrence};
use lang::model::{Document, end_position};
use lsp_types::{CodeActionKind, Range, TextEdit};
use std::path::Path;

/// How a host prefers to offer completing or reopening a task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskToggle {
    /// A command the host executes, listed with the row's other controls.
    Command,
    /// An edit the host applies itself, shown disabled when it is blocked.
    Action,
}

/// Every action offered for one range, in the order a host should show them.
pub(crate) fn code_actions(
    request: &lang::eval::RequestContext<'_>,
    path: &Path,
    range: Range,
    capabilities: Capabilities,
    toggle: TaskToggle,
) -> Vec<CodeActionItem> {
    let ws = request.workspace();
    let Some(doc) = ws.documents().get(path) else {
        return vec![];
    };
    let row = range.start.line as usize;
    let mut result: Vec<_> = providers::controls(request, path, row, toggle, capabilities)
        .into_iter()
        .map(CodeActionItem::command)
        .collect();
    if toggle == TaskToggle::Action
        && let Some(i) = doc.tasks.iter().position(|t| t.line == row)
    {
        let title = rows::task_toggle_title(&mut request.engine(), path, i);
        let mut item = CodeActionItem::edit(title, CodeActionKind::REFACTOR_REWRITE, vec![]);
        match toggle_task(request, path, i) {
            Ok(edits) => item.edits = edits,
            Err(reason) => item.disabled = Some(reason),
        }
        result.push(item);
    }
    result.extend(refactors(request, path, range));
    let dates: Vec<_> = freeze_dates(request, path)
        .into_iter()
        .filter(|e| e.range.start.line >= range.start.line && e.range.start.line <= range.end.line)
        .collect();
    if !dates.is_empty() {
        result.push(CodeActionItem::edit(
            "Freeze relative date".into(),
            CodeActionKind::REFACTOR_REWRITE,
            dates,
        ));
    }
    if row == 0
        && doc.line(0).trim_start().starts_with('#')
        && capabilities.supports(&Action::ShowToday)
    {
        result.push(CodeActionItem::command(
            Action::ShowToday.command(rows::titled(&mut request.engine(), "flag", "today")),
        ));
    }
    result
}

pub(crate) fn toggle_task(
    request: &lang::eval::RequestContext<'_>,
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
            .any(|i| doc.tasks[*i].attributes.contains_key("every"))
    {
        return Err(
            "Complete recurring tasks individually; recurring parent tasks are unsupported".into(),
        );
    }
    match task.attributes.get("every") {
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
    if task.checked {
        return Err("A recurring task should remain unchecked; remove [x] to resume it".into());
    }
    let due = task
        .attributes
        .get("due")
        .map(|a| engine.when(path, &a.value).and_then(|v| engine.date(&v)))
        .transpose()?
        .unwrap_or(today);
    let anchor = task
        .attributes
        .get("repeat_from")
        .map(|a| NaiveDate::parse_from_str(&a.value, "%Y-%m-%d").map_err(|e| e.to_string()))
        .transpose()?
        .unwrap_or(due);
    let next = next_occurrence(recurrence, anchor, today.max(due))?;
    let mut edits = Vec::new();
    if let Some(attr) = task.attributes.get("due") {
        edits.push(TextEdit::new(
            attr.value_span.range(&doc.text),
            next.to_string(),
        ));
    }
    let mut suffix = String::new();
    if !task.attributes.contains_key("due") {
        suffix.push_str(&format!(" @due({next})"));
    }
    if !task.attributes.contains_key("repeat_from") {
        suffix.push_str(&format!(" @repeat_from({anchor})"));
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
            span.range(&doc.text),
            if done { " ".into() } else { "x".into() },
        ));
        if let Some(attr) = task.attributes.get("completed") {
            edits.push(TextEdit::new(
                attr.span.range(&doc.text),
                if done {
                    String::new()
                } else {
                    format!("@completed({today})")
                },
            ));
        } else if !done {
            edits.push(TextEdit::new(
                Range::new(doc.line_end(task.line), doc.line_end(task.line)),
                format!(" @completed({today})"),
            ));
        }
    }
    edits
}
pub(crate) fn freeze_dates(request: &lang::eval::RequestContext<'_>, path: &Path) -> Vec<TextEdit> {
    let today = request.today();
    let workspace = request.workspace();

    let doc = &workspace.documents()[path];
    let mut engine = request.engine();
    doc.tasks
        .iter()
        .flat_map(|t| t.attributes.iter())
        .chain(doc.events.iter().flat_map(|e| e.attributes.iter()))
        .filter(|(key, _)| matches!(key.as_str(), "due" | "scheduled" | "at"))
        .filter_map(|(_, a)| {
            lang::eval::engine::relative_date(&a.value, today)
                .and_then(|_| engine.when(path, &a.value).ok())
                .map(|v| TextEdit::new(a.value_span.range(&doc.text), v.display()))
        })
        .collect()
}
