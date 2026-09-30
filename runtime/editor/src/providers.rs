//! One extension point for everything a note shows beyond its text: inlays,
//! hovers, diagnostics, the controls on a row and formatting edits. The
//! editor's own features and every .xmd feature module are providers of the
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
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

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
        .or_else(|| analysis::hover_at(request, path, at, catalog::task_hover))
}
/// The editor's own diagnostics, then the feature modules', in source order.
pub(crate) fn diagnostics(
    request: &RequestContext<'_>,
    path: &Path,
    editing: bool,
) -> Vec<Diagnostic> {
    let mut result = analysis::collect_native(request, path, editing);
    result.extend(modules(request).flat_map(|m| module::diagnostics(m, request, path)));
    result.extend(modules(request).flat_map(|m| module::control_problems(m, request, path)));
    // A stdlib presentation that fails on the note's own records shows its
    // fallback where it is drawn; the reason is a warning here.
    result.extend(catalog::presentations(request, path));
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
    let mut proposed = proposals(request, path, capabilities);
    row_commands(request, path, row, &mut proposed, toggle, capabilities)
}
/// Each row's commands, in row order: every row in `rows` and every row a
/// module proposes an action for.
pub(crate) fn row_controls(
    request: &RequestContext<'_>,
    path: &Path,
    mut rows: BTreeSet<usize>,
    toggle: TaskToggle,
    capabilities: Capabilities,
) -> Vec<(usize, Command)> {
    let mut proposed = proposals(request, path, capabilities);
    rows.extend(proposed.iter().flat_map(|lines| lines.keys()));
    rows.into_iter()
        .flat_map(|row| {
            row_commands(request, path, row, &mut proposed, toggle, capabilities)
                .into_iter()
                .map(move |command| (row, command))
        })
        .collect()
}
/// Every feature module's proposed commands by row, one call per module for
/// the whole note.
fn proposals(
    request: &RequestContext<'_>,
    path: &Path,
    capabilities: Capabilities,
) -> Vec<BTreeMap<usize, Vec<Command>>> {
    modules(request)
        .map(|m| module::controls(m, request, path, capabilities))
        .collect()
}
/// One row's own controls, then what each module proposed for it, taken out
/// of `proposed`.
fn row_commands(
    request: &RequestContext<'_>,
    path: &Path,
    row: usize,
    proposed: &mut [BTreeMap<usize, Vec<Command>>],
    toggle: TaskToggle,
    capabilities: Capabilities,
) -> Vec<Command> {
    let mut commands = rows::builtin_controls(request, path, row, toggle, capabilities);
    commands.extend(
        proposed
            .iter_mut()
            .flat_map(|lines| lines.remove(&row).unwrap_or_default()),
    );
    commands
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
