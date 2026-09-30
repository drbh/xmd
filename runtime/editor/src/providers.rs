//! One extension point for everything a note shows beyond its text: inlays,
//! hovers, diagnostics, the controls on a row and formatting edits. The
//! editor's own features and every .xmd feature module are providers of the
//! same kinds, so a feature can move between Rust and a module without any
//! caller noticing. Each function below is the one place its kind's order is
//! decided, and this is the one module that calls down into providers;
//! nothing calls back up into it.
//!
//! Controls have one pipeline. Every provider proposes its controls for the
//! whole note at once (`tasks`, `resources` and `lookups` natively, each
//! feature module through its `actions` hook), each control is an action of
//! a kind exactly one provider owns, and that owner prepares it, whoever
//! proposed it: when a module's proposal is checked, and again when a host
//! executes it.
use crate::commands::{
    Action, ActionProvider, Ask, Capabilities, CommandId, Direct, Prepared, PreparedAction, Rows,
    TaskToggle,
};
use crate::inlays::{self, InlayOutput};
use crate::lookups::Lookups;
use crate::modules::{self as module, Invocations};
use crate::resources::Resources;
use crate::tasks::Tasks;
use lang::eval::modules::{Module, ModuleKind};
use lsp_types::{CodeLens, Command, Diagnostic, Hover, Position, Range, TextEdit};
use std::{collections::BTreeMap, path::Path};

/// Every active feature module, in manifest order.
fn modules<'a>(request: &crate::Request<'a>) -> impl Iterator<Item = &'a Module> {
    request.workspace().modules().of_kind(ModuleKind::Feature)
}

/// The built-in providers that propose controls, in the order their
/// controls show on a row; the feature modules' follow them.
const PROPOSING: [&dyn ActionProvider; 3] = [&Tasks, &Resources, &Lookups];
/// Every provider that owns an action kind.
const OWNERS: [&dyn ActionProvider; 5] = [&Tasks, &Resources, &Lookups, &Direct, &Invocations];

/// The provider that prepares `action`'s kind.
fn owner(action: &Action) -> &'static dyn ActionProvider {
    let kind = CommandId::from(action);
    OWNERS
        .into_iter()
        .find(|provider| provider.kinds().contains(&kind))
        .expect("every action kind has an owner")
}

/// Validate `action` against the current workspace and execution-time clock.
/// The host still owns version checks, applying edits, opening URLs and
/// refreshing data. An invocation prepares as the action its module's
/// reducer returns.
pub(crate) fn prepare(
    request: &crate::Request<'_>,
    action: &Action,
    capabilities: Capabilities,
) -> Result<PreparedAction, String> {
    action.admit(request, capabilities)?;
    match owner(action).prepare(request, action, capabilities)? {
        Prepared::Effect(effect) => Ok(effect),
        Prepared::Reduced(reduced) => {
            reduced.admit(request, capabilities)?;
            match owner(&reduced).prepare(request, &reduced, capabilities)? {
                Prepared::Effect(effect) => Ok(effect),
                Prepared::Reduced(_) => Err("A reducer must return a concrete action".into()),
            }
        }
    }
}
/// Whether an action a module proposed would prepare, as its owner judges.
fn check(
    request: &crate::Request<'_>,
    action: &Action,
    capabilities: Capabilities,
) -> Result<(), String> {
    action.admit(request, capabilities)?;
    owner(action).check(request, action, capabilities)
}

/// Inline labels come only from feature modules.
pub(crate) fn hints(request: &crate::Request<'_>, path: &Path, range: Range) -> InlayOutput {
    inlays::run(request, path, range, true, |context, output| {
        for m in modules(request) {
            module::inlays(m, context, output);
        }
    })
}
/// Whether any label in the note reads the clock, so a host knows to refresh.
/// The providers run as for [`hints`], but no label is drawn.
pub(crate) fn live_hints(request: &crate::Request<'_>, path: &Path) -> bool {
    let everywhere = Range::new(Position::new(0, 0), Position::new(u32::MAX, 0));
    inlays::run(request, path, everywhere, false, |context, output| {
        for m in modules(request) {
            module::inlays(m, context, output);
        }
    })
    .time_dependent
}
/// A feature module's hover takes precedence, so a module can refine what the
/// editor would otherwise explain.
pub(crate) fn hover(request: &crate::Request<'_>, path: &Path, at: Position) -> Option<Hover> {
    request.workspace().documents().get(path)?;
    modules(request)
        .find_map(|m| module::hover(m, request, path, at))
        .or_else(|| {
            analysis::hover_at(request, path, at, &|context, path, index| {
                catalog::task_hover(context, &request.records, path, index)
            })
        })
}
/// The editor's own diagnostics, then the feature modules', in source order.
pub(crate) fn diagnostics(
    request: &crate::Request<'_>,
    path: &Path,
    editing: bool,
) -> Vec<Diagnostic> {
    let mut result = analysis::collect_native(request, path, editing);
    result.extend(modules(request).flat_map(|m| module::diagnostics(m, request, path)));
    // Why a module's controls are withheld. Like the stdlib presentation
    // checks, only for a workspace with modules of its own.
    if !request.workspace().modules().only_bundled() {
        let capabilities = Capabilities::NATIVE;
        result.extend(modules(request).flat_map(|m| {
            let controls = module::controls(m, request, path, capabilities, &|action| {
                check(request, action, capabilities)
            });
            module::control_problems(m, controls)
        }));
    }
    // A stdlib presentation that fails on the note's own records shows its
    // fallback where it is drawn; the reason is a warning here.
    result.extend(catalog::presentations(request, &request.records, path));
    result.sort_by_key(|d| (d.range.start, d.range.end, d.message.clone()));
    result.dedup_by(|a, b| a.range == b.range && a.message == b.message);
    result
}
/// Every control on `ask.rows`, by row: each built-in provider's, then each
/// feature module's that passed its owners' checks. Every provider is asked
/// once for the whole note.
pub(crate) fn controls(
    request: &crate::Request<'_>,
    path: &Path,
    ask: Ask,
) -> BTreeMap<usize, Vec<Command>> {
    let mut rows: BTreeMap<usize, Vec<Command>> = BTreeMap::new();
    for provider in PROPOSING {
        for proposal in provider.propose(request, path, ask) {
            if ask.capabilities.supports(&proposal.action) {
                rows.entry(proposal.line)
                    .or_default()
                    .push(proposal.action.command(proposal.title));
            }
        }
    }
    for m in modules(request) {
        let lines = module::controls(m, request, path, ask.capabilities, &|action| {
            check(request, action, ask.capabilities)
        });
        for (line, commands) in lines.unwrap_or_default() {
            if let (true, Ok(commands)) = (ask.rows.has(line), commands) {
                rows.entry(line).or_default().extend(commands);
            }
        }
    }
    rows
}
/// The commands one row offers. The task toggle is among them only when the
/// host takes it as a command; as an action it is a code action of its own.
pub(crate) fn row_controls(
    request: &crate::Request<'_>,
    path: &Path,
    row: usize,
    toggle: TaskToggle,
    capabilities: Capabilities,
) -> Vec<Command> {
    let ask = Ask {
        rows: Rows::One(row),
        toggle,
        capabilities,
    };
    controls(request, path, ask)
        .remove(&row)
        .unwrap_or_default()
}
/// A lens for every control in the note, in row order.
pub(crate) fn lenses(
    request: &crate::Request<'_>,
    path: &Path,
    capabilities: Capabilities,
) -> Vec<CodeLens> {
    let ask = Ask {
        rows: Rows::All,
        toggle: TaskToggle::Command,
        capabilities,
    };
    controls(request, path, ask)
        .into_iter()
        .flat_map(|(row, commands)| {
            let at = Position::new(row as u32, 0);
            commands.into_iter().map(move |command| CodeLens {
                range: Range::new(at, at),
                command: Some(command),
                data: None,
            })
        })
        .collect()
}
/// Table formatting, then every feature module's edits, checked to apply
/// cleanly together.
pub(crate) fn edits(request: &crate::Request<'_>, path: &Path) -> Result<Vec<TextEdit>, String> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use strum::VariantArray;

    /// Routing by kind is a table: each kind has exactly one owner.
    #[test]
    fn every_action_kind_has_one_owner() {
        for kind in CommandId::VARIANTS {
            let owners = OWNERS
                .iter()
                .filter(|provider| provider.kinds().contains(kind))
                .count();
            assert_eq!(owners, 1, "{kind:?} has {owners} owners");
        }
    }
}
