//! One extension point for everything a note shows beyond its text: inlays,
//! hovers, diagnostics, the controls on a row and formatting edits. The
//! editor's own features and every .x.md feature module are providers of the
//! same kinds, so a feature can move between Rust and a module without any
//! caller noticing. Each function below is the one place its kind's order is
//! decided.
use crate::code_actions::TaskToggle;
use crate::commands::Capabilities;
use crate::inlays::{InlayContext, InlaySink};
use crate::modules as module;
use crate::rows;
use lang::eval::RequestContext;
use lang::eval::modules::{Module, ModuleKind};
use lsp_types::{Command, Diagnostic, Hover, Position, TextEdit};
use std::path::Path;

/// Every active feature module, in manifest order.
fn modules<'a>(request: &RequestContext<'a>) -> impl Iterator<Item = &'a Module> {
    request.workspace().modules().of_kind(ModuleKind::Feature)
}

/// Inline labels come only from feature modules.
pub(crate) fn inlays(
    request: &RequestContext<'_>,
    context: &mut InlayContext<'_, '_>,
    output: &mut InlaySink,
) {
    for m in modules(request) {
        module::inlays(m, context, output);
    }
}
/// A feature module's hover takes precedence, so a module can refine what the
/// editor would otherwise explain.
pub(crate) fn hover(request: &RequestContext<'_>, path: &Path, at: Position) -> Option<Hover> {
    request.workspace().documents().get(path)?;
    modules(request)
        .find_map(|m| module::hover(m, request, path, at))
        .or_else(|| analysis::hover_at(request, path, at))
}
/// The editor's own diagnostics, then the feature modules', in source order.
pub(crate) fn diagnostics(
    request: &RequestContext<'_>,
    path: &Path,
    editing: bool,
) -> Vec<Diagnostic> {
    let mut result = analysis::collect_native(request, path, editing);
    result.extend(modules(request).flat_map(|m| module::diagnostics(m, request, path)));
    result.sort_by_key(|d| (d.range.start, d.range.end, d.message.clone()));
    result.dedup_by(|a, b| a.range == b.range && a.message == b.message);
    result
}
/// The commands one row offers. The task toggle is among them only when the
/// host takes it as a command; as an action it is a code action of its own.
pub(crate) fn controls(
    request: &RequestContext<'_>,
    path: &Path,
    row: usize,
    toggle: TaskToggle,
    capabilities: Capabilities,
) -> Vec<Command> {
    row_controls(request, path, vec![row], toggle, capabilities)
        .into_iter()
        .map(|(_, command)| command)
        .collect()
}
/// Each row's commands, rows in the order given. A module's input is the same
/// for every row but the row itself, so it is gathered once per note.
pub(crate) fn row_controls(
    request: &RequestContext<'_>,
    path: &Path,
    rows: Vec<usize>,
    toggle: TaskToggle,
    capabilities: Capabilities,
) -> Vec<(usize, Command)> {
    if rows.is_empty() {
        return vec![];
    }
    let inputs: Vec<_> = modules(request)
        .filter_map(|m| Some((m, module::actions_input(m, request, path, capabilities)?)))
        .collect();
    let mut result = vec![];
    for row in rows {
        let builtin = rows::builtin_controls(request, path, row, toggle, capabilities);
        let proposed = inputs
            .iter()
            .flat_map(|(m, input)| module::controls(m, input, request, row, capabilities));
        result.extend(
            builtin
                .into_iter()
                .chain(proposed)
                .map(|command| (row, command)),
        );
    }
    result
}
/// Table formatting, then every feature module's edits, checked to apply
/// cleanly together.
pub(crate) fn edits(request: &RequestContext<'_>, path: &Path) -> Result<Vec<TextEdit>, String> {
    let doc = request
        .workspace()
        .documents()
        .get(path)
        .ok_or("Unknown document")?;
    let mut edits = lang::eval::tables::formatting(doc);
    for m in modules(request) {
        edits.extend(module::edits(m, request, path)?);
    }
    lang::model::apply_edits(&doc.text, &edits)?;
    edits.sort_by_key(|e| (e.range.start, e.range.end));
    Ok(edits)
}
