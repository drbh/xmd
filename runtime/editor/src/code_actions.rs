//! Code actions: what a host offers over a range. The row's controls come
//! from every provider; the task toggle as an edit, refactors, freezing
//! relative dates and showing today are offered here.
use crate::commands::{Action, Capabilities, TaskToggle, titled};
use crate::{providers, tasks};
use analysis::{CodeActionItem, refactors};
use lang::syntax::{AttributeKey, AttributeValue};
use lsp_types::{CodeActionKind, Range, TextEdit};
use std::path::Path;

/// Every action offered for one range, in the order a host should show them.
pub(crate) fn code_actions(
    request: &crate::Request<'_>,
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
    let mut result: Vec<_> = providers::row_controls(request, path, row, toggle, capabilities)
        .into_iter()
        .map(CodeActionItem::command)
        .collect();
    if toggle == TaskToggle::Action
        && let Some(i) = doc.tasks.iter().position(|t| t.line == row)
    {
        let title = tasks::title(&mut request.engine(), path, i);
        let mut item = CodeActionItem::edit(title, CodeActionKind::REFACTOR_REWRITE, vec![]);
        match tasks::toggle(request, path, i) {
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
        result.push(CodeActionItem::command(Action::ShowToday.command(titled(
            &mut request.engine(),
            "flag",
            "today",
        ))));
    }
    result
}

pub(crate) fn freeze_dates(request: &crate::Request<'_>, path: &Path) -> Vec<TextEdit> {
    let today = request.today();
    let workspace = request.workspace();

    let doc = &workspace.documents()[path];
    let mut engine = request.engine();
    doc.tasks
        .iter()
        .flat_map(|t| t.attributes.iter())
        .chain(doc.events.iter().flat_map(|e| e.attributes.iter()))
        .filter(|(key, _)| {
            key.parse::<AttributeKey>()
                .is_ok_and(|k| k.value() == AttributeValue::When)
        })
        .filter_map(|(_, a)| {
            lang::eval::engine::relative_date(&a.value, today)
                .and_then(|_| engine.when(path, &a.value).ok())
                .map(|v| TextEdit::new(a.value_span.range(doc), v.display()))
        })
        .collect()
}
