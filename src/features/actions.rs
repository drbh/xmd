use crate::{
    document::{Span, byte_at},
    engine::{Engine, Value, next_occurrence},
    workspace::Workspace,
};
use chrono::NaiveDate;
use lsp_types::{Position, Range, TextEdit};
use std::path::Path;

pub fn toggle_task(
    workspace: &Workspace,
    path: &Path,
    index: usize,
    today: NaiveDate,
) -> Result<Vec<TextEdit>, String> {
    let doc = &workspace.documents[path];
    let task = doc.tasks.get(index).ok_or("No task at this line")?;
    let mut engine = Engine::new(workspace, today);
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
    if let Some(recurrence) = task.attributes.get("every") {
        if task.checked {
            return Err("A recurring task should remain unchecked; remove [x] to resume it".into());
        }
        let due = task
            .attributes
            .get("due")
            .map(|a| engine.when(path, &a.value).and_then(|v| v.date()))
            .transpose()?
            .unwrap_or(today);
        let anchor = task
            .attributes
            .get("repeat_from")
            .map(|a| NaiveDate::parse_from_str(&a.value, "%Y-%m-%d").map_err(|e| e.to_string()))
            .transpose()?
            .unwrap_or(due);
        let next = next_occurrence(&recurrence.value, anchor, today.max(due))?;
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
pub fn freeze_dates(workspace: &Workspace, path: &Path, today: NaiveDate) -> Vec<TextEdit> {
    let doc = &workspace.documents[path];
    let mut engine = Engine::new(workspace, today);
    doc.tasks
        .iter()
        .flat_map(|t| t.attributes.iter())
        .chain(doc.events.iter().flat_map(|e| e.attributes.iter()))
        .filter(|(key, _)| matches!(key.as_str(), "due" | "scheduled" | "at"))
        .filter_map(|(_, a)| {
            crate::engine::relative_date(&a.value, today)
                .and_then(|_| engine.when(path, &a.value).ok())
                .map(|v| TextEdit::new(a.value_span.range(&doc.text), v.display()))
        })
        .collect()
}
pub fn end_position(text: &str) -> Position {
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
pub fn value_date(value: Value) -> Result<NaiveDate, String> {
    value.date()
}
