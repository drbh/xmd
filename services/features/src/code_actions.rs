//! Code actions: what a host offers over a range. The row's controls come
//! from every provider, its row actions resolved into edits up front when
//! the host prefers; refactors, freezing relative dates and showing today
//! are offered here.
use crate::commands::{
    Action, Capabilities, Control, PreparedAction, Proposal, RowActions, titled,
};
use crate::providers;
use analysis::{CodeActionItem, refactors};
use lang::syntax::AttributeValue;
use lsp_types::{CodeActionKind, Range, TextEdit};
use std::path::Path;

/// Every action offered for one range, in the order a host should show them.
pub(crate) fn code_actions(
    request: &crate::Request<'_>,
    path: &Path,
    range: Range,
    capabilities: Capabilities,
    row_actions: RowActions,
) -> Vec<CodeActionItem> {
    let ws = request.workspace();
    let Some(doc) = ws.documents().get(path) else {
        return vec![];
    };
    let row = range.start.line as usize;
    let mut result = Vec::new();
    let mut resolved = Vec::new();
    for control in providers::row_controls(request, path, row, capabilities) {
        let is_row = matches!(control.proposal.action, Action::Row { .. });
        if is_row && row_actions == RowActions::Edit {
            resolved.push(resolve(request, control, capabilities));
        } else if control.disabled.is_none() {
            let Proposal { title, action, .. } = control.proposal;
            result.push(CodeActionItem::command(action.command(title)));
        }
    }
    result.extend(resolved);
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

/// A row action as the edit it comes to now, or disabled with the reason it
/// cannot run. One whose reducer answers with something other than an edit
/// stays a command.
fn resolve(
    request: &crate::Request<'_>,
    control: Control,
    capabilities: Capabilities,
) -> CodeActionItem {
    let Proposal { title, action, .. } = control.proposal;
    let mut item = CodeActionItem::edit(title.clone(), CodeActionKind::REFACTOR_REWRITE, vec![]);
    if let Some(reason) = control.disabled {
        item.disabled = Some(reason);
        return item;
    }
    match providers::prepare(request, &action, capabilities) {
        Ok(PreparedAction::Edit { edits, .. }) => item.edits = edits,
        Ok(_) => return CodeActionItem::command(action.command(title)),
        Err(reason) => item.disabled = Some(reason),
    }
    item
}

pub(crate) fn freeze_dates(request: &crate::Request<'_>, path: &Path) -> Vec<TextEdit> {
    let today = request.today();
    let workspace = request.workspace();

    let doc = &workspace.documents()[path];
    let mut engine = request.engine();
    doc.claimed_attributes()
        .filter(|(key, _)| doc.attribute_value(key) == Some(AttributeValue::When))
        .filter_map(|(_, a)| {
            lang::syntax::relative_date(&a.value, today)
                .and_then(|_| engine.when(path, &a.value).ok())
                .map(|v| TextEdit::new(a.value_span.range(doc), v.display()))
        })
        .collect()
}
