use crate::commands_impl::{Action, Capabilities};
use chrono::NaiveDate;
use common::Span;
use eval::engine::next_occurrence;
use lsp_types::{CodeActionKind, Command, Position, Range, TextEdit};
use model::byte_at;
use std::path::Path;

/// One proposal, before a host decides how to carry it: an edit, a command, or
/// an edit it must show as unavailable with a reason.
#[derive(Clone, Debug)]
pub struct CodeActionItem {
    pub title: String,
    pub kind: Option<CodeActionKind>,
    pub edits: Vec<TextEdit>,
    pub command: Option<Command>,
    pub disabled: Option<String>,
}
impl CodeActionItem {
    fn edit(title: String, kind: CodeActionKind, edits: Vec<TextEdit>) -> Self {
        Self {
            title,
            kind: Some(kind),
            edits,
            command: None,
            disabled: None,
        }
    }
    fn command(command: Command) -> Self {
        Self {
            title: command.title.clone(),
            kind: None,
            edits: vec![],
            command: Some(command),
            disabled: None,
        }
    }
}

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
    request: &eval::RequestContext<'_>,
    path: &Path,
    range: Range,
    capabilities: Capabilities,
    toggle: TaskToggle,
) -> Vec<CodeActionItem> {
    let ws = request.workspace();
    let Some(doc) = ws.documents.get(path) else {
        return vec![];
    };
    let row = range.start.line as usize;
    let mut result: Vec<_> = crate::interaction::row_commands(
        request,
        path,
        row,
        toggle == TaskToggle::Command,
        capabilities,
    )
    .into_iter()
    .map(CodeActionItem::command)
    .collect();
    if toggle == TaskToggle::Action
        && let Some((i, task)) = doc.tasks.iter().enumerate().find(|(_, t)| t.line == row)
    {
        let title = crate::interaction::task_toggle_title(
            task.attributes.contains_key("every"),
            request.engine().task_done(path, i),
        );
        let mut item = CodeActionItem::edit(title, CodeActionKind::REFACTOR_REWRITE, vec![]);
        match toggle_task(request, path, i) {
            Ok(edits) => item.edits = edits,
            Err(reason) => item.disabled = Some(reason),
        }
        result.push(item);
    }
    for action in crate::refactor::refactors(request, path, range) {
        result.push(CodeActionItem::edit(
            action.title,
            action.kind,
            action.edits,
        ));
    }
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
            Action::ShowToday.command(format!("{} today", eval::glyphs::FLAG)),
        ));
    }
    result
}

pub(crate) fn toggle_task(
    request: &eval::RequestContext<'_>,
    path: &Path,
    index: usize,
) -> Result<Vec<TextEdit>, String> {
    let workspace = request.workspace();
    let today = request.today();

    let doc = &workspace.documents[path];
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
            let blocked = engine.blocked(path, *i).map_err(|e| e.to_string())?;
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
    if let Some(recurrence) = task.attributes.get("every") {
        if task.checked {
            return Err("A recurring task should remain unchecked; remove [x] to resume it".into());
        }
        let due = task
            .attributes
            .get("due")
            .map(|a| {
                engine
                    .when(path, &a.value)
                    .and_then(|v| engine.date(&v))
                    .map_err(|e| e.to_string())
            })
            .transpose()?
            .unwrap_or(today);
        let anchor = task
            .attributes
            .get("repeat_from")
            .map(|a| NaiveDate::parse_from_str(&a.value, "%Y-%m-%d").map_err(|e| e.to_string()))
            .transpose()?
            .unwrap_or(due);
        let next = next_occurrence(&recurrence.value, anchor, today.max(due))
            .map_err(|e| e.to_string())?;
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
                "{prefix}<!-- wtf-history {} -->{newline}",
                history.to_string().replace("-->", "--\\u003e")
            ),
        ));
        return Ok(edits);
    }
    let mut edits = Vec::new();
    for i in indices {
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
    Ok(edits)
}
pub(crate) fn freeze_dates(request: &eval::RequestContext<'_>, path: &Path) -> Vec<TextEdit> {
    let today = request.today();
    let workspace = request.workspace();

    let doc = &workspace.documents[path];
    let mut engine = request.engine();
    doc.tasks
        .iter()
        .flat_map(|t| t.attributes.iter())
        .chain(doc.events.iter().flat_map(|e| e.attributes.iter()))
        .filter(|(key, _)| matches!(key.as_str(), "due" | "scheduled" | "at"))
        .filter_map(|(_, a)| {
            eval::engine::relative_date(&a.value, today)
                .and_then(|_| engine.when(path, &a.value).ok())
                .map(|v| TextEdit::new(a.value_span.range(&doc.text), v.display()))
        })
        .collect()
}
fn end_position(text: &str) -> Position {
    if text.ends_with('\n') {
        Position::new(text.lines().count() as u32, 0)
    } else {
        let lines: Vec<_> = text.lines().collect();
        Position::new(
            lines.len().saturating_sub(1) as u32,
            lines.last().unwrap_or(&"").encode_utf16().count() as u32,
        )
    }
}
pub fn apply_edits(text: &str, edits: &[TextEdit]) -> Result<String, String> {
    fn offset(text: &str, pos: Position) -> Result<usize, String> {
        let mut base = 0;
        for (i, line) in text.split_inclusive('\n').enumerate() {
            if i == pos.line as usize {
                return byte_at(line.trim_end_matches(['\n', '\r']), pos.character)
                    .map(|n| base + n)
                    .ok_or("Invalid UTF-16 character boundary".into());
            }
            base += line.len();
        }
        if pos == end_position(text) {
            Ok(text.len())
        } else {
            Err("Invalid edit position".into())
        }
    }
    let mut replacements = edits
        .iter()
        .map(|e| {
            if e.range.start > e.range.end {
                return Err("Reversed edit range".into());
            }
            Ok((
                offset(text, e.range.start)?,
                offset(text, e.range.end)?,
                e.new_text.clone(),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    replacements.sort_by_key(|(start, end, _)| (*start, *end));
    for pair in replacements.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err("Overlapping edits".into());
        }
    }
    let mut result = text.to_string();
    for (start, end, new) in replacements.into_iter().rev() {
        result.replace_range(start..end, &new);
    }
    Ok(result)
}
