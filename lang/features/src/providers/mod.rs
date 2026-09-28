//! One extension point for everything a note shows beyond its text: inlays,
//! hovers, diagnostics, the controls on a row and formatting edits. The
//! editor's own features and every .x.md feature module are providers of the
//! same kinds, so a feature can move between Rust and a module without any
//! caller noticing. Each function below is the one place its kind's order is
//! decided.
mod module;

pub(crate) use module::reduce;

use crate::controls::commands::Capabilities;
use crate::view::inlays::{InlayContext, InlaySink};
use eval::RequestContext;
use eval::modules::{Module, ModuleKind};
use lsp_types::{Command, Diagnostic, Hover, Position, TextEdit};
use std::path::Path;

pub(crate) trait Provider {
    fn inlays(&self, _context: &mut InlayContext<'_, '_>, _output: &mut InlaySink) {}
    fn hover(&self, _request: &RequestContext<'_>, _path: &Path, _at: Position) -> Option<Hover> {
        None
    }
    fn diagnostics(
        &self,
        _request: &RequestContext<'_>,
        _path: &Path,
        _editing: bool,
    ) -> Vec<Diagnostic> {
        Vec::new()
    }
    /// The commands one row offers; `include_task` adds the task toggle, which
    /// a code action already offers on its own.
    fn controls(
        &self,
        _request: &RequestContext<'_>,
        _path: &Path,
        _row: usize,
        _include_task: bool,
        _capabilities: Capabilities,
    ) -> Vec<Command> {
        Vec::new()
    }
    fn edits(&self, _request: &RequestContext<'_>, _path: &Path) -> Result<Vec<TextEdit>, String> {
        Ok(Vec::new())
    }
}

/// The editor's own features.
pub(crate) struct Builtin;
impl Provider for Builtin {
    fn hover(&self, request: &RequestContext<'_>, path: &Path, at: Position) -> Option<Hover> {
        crate::language::hover::hover_at(request, path, at)
    }
    fn diagnostics(
        &self,
        request: &RequestContext<'_>,
        path: &Path,
        editing: bool,
    ) -> Vec<Diagnostic> {
        crate::language::diagnostics::collect_native(request, path, editing)
    }
    fn controls(
        &self,
        request: &RequestContext<'_>,
        path: &Path,
        row: usize,
        include_task: bool,
        capabilities: Capabilities,
    ) -> Vec<Command> {
        crate::controls::rows::builtin_controls(request, path, row, include_task, capabilities)
    }
    fn edits(&self, request: &RequestContext<'_>, path: &Path) -> Result<Vec<TextEdit>, String> {
        let doc = request
            .workspace()
            .documents
            .get(path)
            .ok_or("Unknown document")?;
        Ok(eval::tables::formatting(doc))
    }
}

/// Every active feature module, in manifest order.
fn modules<'a>(request: &RequestContext<'a>) -> impl Iterator<Item = &'a Module> {
    request
        .workspace()
        .modules
        .active()
        .filter(|m| m.kind == ModuleKind::Feature)
}
/// The editor's own features, then the feature modules.
fn all<'a>(request: &RequestContext<'a>) -> impl Iterator<Item = &'a dyn Provider> {
    std::iter::once(&Builtin as &dyn Provider).chain(modules(request).map(|m| m as &dyn Provider))
}

pub(crate) fn inlays(
    request: &RequestContext<'_>,
    context: &mut InlayContext<'_, '_>,
    output: &mut InlaySink,
) {
    for provider in all(request) {
        provider.inlays(context, output);
    }
}
/// A feature module's hover takes precedence, so a module can refine what the
/// editor would otherwise explain.
pub(crate) fn hover(request: &RequestContext<'_>, path: &Path, at: Position) -> Option<Hover> {
    request.workspace().documents.get(path)?;
    modules(request)
        .find_map(|m| m.hover(request, path, at))
        .or_else(|| Builtin.hover(request, path, at))
}
pub(crate) fn diagnostics(
    request: &RequestContext<'_>,
    path: &Path,
    editing: bool,
) -> Vec<Diagnostic> {
    let mut result: Vec<_> = all(request)
        .flat_map(|p| p.diagnostics(request, path, editing))
        .collect();
    result.sort_by_key(|d| (d.range.start, d.range.end, d.message.clone()));
    result.dedup_by(|a, b| a.range == b.range && a.message == b.message);
    result
}
pub(crate) fn controls(
    request: &RequestContext<'_>,
    path: &Path,
    row: usize,
    include_task: bool,
    capabilities: Capabilities,
) -> Vec<Command> {
    all(request)
        .flat_map(|p| p.controls(request, path, row, include_task, capabilities))
        .collect()
}
/// Every provider's edits, checked to apply cleanly together.
pub(crate) fn edits(request: &RequestContext<'_>, path: &Path) -> Result<Vec<TextEdit>, String> {
    let doc = request
        .workspace()
        .documents
        .get(path)
        .ok_or("Unknown document")?;
    let mut edits = Vec::new();
    for provider in all(request) {
        edits.extend(provider.edits(request, path)?);
    }
    crate::controls::code_actions::apply_edits(&doc.text, &edits)?;
    edits.sort_by_key(|e| (e.range.start, e.range.end));
    Ok(edits)
}
