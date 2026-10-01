//! One extension point for everything a note shows beyond its text: inlays,
//! hovers, diagnostics, the controls on a row and formatting edits. The
//! editor's own features and every .xmd feature module are providers of the
//! same kinds, so a feature can move between Rust and a module without any
//! caller noticing. Each function below is the one place its kind's order is
//! decided, and this is the one module that calls down into providers;
//! nothing calls back up into it.
//!
//! Controls have one pipeline. Every provider proposes its controls for the
//! whole note at once (`resources` and `lookups` natively, each feature
//! module through its `actions` hook), each control is an action of
//! a kind exactly one provider owns, and that owner prepares it, whoever
//! proposed it: when a module's proposal is checked, and again when a host
//! executes it.
use crate::commands::{
    Action, ActionProvider, Ask, Capabilities, CommandId, Control, Direct, Prepared,
    PreparedAction, Rows,
};
use crate::inlays::{self, InlayOutput};
use crate::lookups::Lookups;
use crate::modules::{self as module, Invocations};
use crate::resources::Resources;
use lang::eval::modules::{Module, ModuleKind};
use lsp_types::{CodeLens, Diagnostic, DocumentSymbol, Hover, Position, Range, TextEdit};
use std::{collections::BTreeMap, path::Path};

/// Every active feature module, in manifest order.
fn modules<'a>(request: &crate::Request<'a>) -> impl Iterator<Item = &'a Module> {
    request.workspace().modules().of_kind(ModuleKind::Feature)
}

/// The built-in providers that propose controls, in the order their
/// controls show on a row; the feature modules' follow them.
const PROPOSING: [&dyn ActionProvider; 2] = [&Resources, &Lookups];
/// Every provider that owns an action kind.
const OWNERS: [&dyn ActionProvider; 4] = [&Resources, &Lookups, &Direct, &Invocations];

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
    labels(request, path, range, true)
}
/// Whether any label in the note reads the clock, so a host knows to refresh.
/// The providers run as for [`hints`], but no label is drawn.
pub(crate) fn live_hints(request: &crate::Request<'_>, path: &Path) -> bool {
    let everywhere = Range::new(Position::new(0, 0), Position::new(u32::MAX, 0));
    labels(request, path, everywhere, false).time_dependent
}
/// Every feature module's labels over `range`, drawn or only weighed for
/// whether any moves with the clock.
fn labels(request: &crate::Request<'_>, path: &Path, range: Range, drawn: bool) -> InlayOutput {
    inlays::run(request, path, range, drawn, |context, output| {
        for m in modules(request) {
            module::inlays(m, context, output);
        }
    })
}
/// A feature module's hover takes precedence, so a module can refine what the
/// editor would otherwise explain; a module's fallback hover answers only
/// where the editor finds nothing more specific than the row.
pub(crate) fn hover(request: &crate::Request<'_>, path: &Path, at: Position) -> Option<Hover> {
    request.workspace().documents().get(path)?;
    modules(request)
        .find_map(|m| module::hover(m, request, path, at, false))
        .or_else(|| {
            analysis::hover_at(request, path, at, &|| {
                modules(request).find_map(|m| module::hover(m, request, path, at, true))
            })
        })
}
/// The note's outline: the editor's own entries and the feature modules'.
pub(crate) fn symbols(request: &crate::Request<'_>, path: &Path) -> Vec<DocumentSymbol> {
    let outlined = modules(request)
        .flat_map(|m| module::symbols(m, request, path))
        .collect();
    analysis::document_symbols(request, path, outlined)
}
/// The editor's own diagnostics, then the feature modules', in source order.
pub(crate) fn diagnostics(
    request: &crate::Request<'_>,
    path: &Path,
    editing: bool,
) -> Vec<Diagnostic> {
    let mut result = analysis::collect_native(request, path, editing);
    result.extend(modules(request).flat_map(|m| module::diagnostics(m, request, path)));
    // A module whose `records` failed built nothing for the note.
    result.extend(catalog::build_problems(request, &request.records, path));
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
/// once for the whole note. A disabled control is kept, with its reason.
pub(crate) fn controls(
    request: &crate::Request<'_>,
    path: &Path,
    ask: Ask,
) -> BTreeMap<usize, Vec<Control>> {
    let mut rows: BTreeMap<usize, Vec<Control>> = BTreeMap::new();
    for provider in PROPOSING {
        for proposal in provider.propose(request, path, ask) {
            if ask.capabilities.supports(&proposal.action) {
                rows.entry(proposal.line).or_default().push(Control {
                    proposal,
                    disabled: None,
                });
            }
        }
    }
    for m in modules(request) {
        let lines = module::controls(m, request, path, ask.capabilities, &|action| {
            check(request, action, ask.capabilities)
        });
        for (line, proposals) in lines.unwrap_or_default() {
            if let (true, Ok(proposals)) = (ask.rows.has(line), proposals) {
                rows.entry(line).or_default().extend(proposals);
            }
        }
    }
    // A row action is the row's own control, what its checkbox does, so it
    // leads the row's controls; the others keep their order.
    for controls in rows.values_mut() {
        controls.sort_by_key(|c| !matches!(c.proposal.action, Action::Row { .. }));
    }
    rows
}
/// The controls one row offers, disabled ones included.
pub(crate) fn row_controls(
    request: &crate::Request<'_>,
    path: &Path,
    row: usize,
    capabilities: Capabilities,
) -> Vec<Control> {
    let ask = Ask {
        rows: Rows::One(row),
        capabilities,
    };
    controls(request, path, ask)
        .remove(&row)
        .unwrap_or_default()
}
/// A lens for every control in the note that can run, in row order.
pub(crate) fn lenses(
    request: &crate::Request<'_>,
    path: &Path,
    capabilities: Capabilities,
) -> Vec<CodeLens> {
    let ask = Ask {
        rows: Rows::All,
        capabilities,
    };
    controls(request, path, ask)
        .into_iter()
        .flat_map(|(row, controls)| {
            let at = Position::new(row as u32, 0);
            controls
                .into_iter()
                .filter(|c| c.disabled.is_none())
                .map(move |c| CodeLens {
                    range: Range::new(at, at),
                    command: Some(c.proposal.action.command(c.proposal.title)),
                    data: None,
                })
        })
        .collect()
}
/// Every feature module's edits, checked to apply cleanly together. `at` is
/// where a `|` was just typed when formatting as the note is typed.
pub(crate) fn edits(
    request: &crate::Request<'_>,
    path: &Path,
    at: Option<Position>,
) -> Result<Vec<TextEdit>, String> {
    let doc = request
        .workspace()
        .documents()
        .get(path)
        .ok_or("Unknown document")?;
    let mut edits = Vec::new();
    for m in modules(request) {
        edits.extend(module::edits(m, request, path, at)?);
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
