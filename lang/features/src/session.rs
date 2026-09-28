//! The feature API. Every editor feature is a method on one request, so a host
//! only ever locks its state, builds a `RequestContext`, calls one method and
//! converts the result to its transport. The implementations stay in the
//! feature module each belongs to; this file is the list of what exists.
use crate::{
    actions_impl::{CodeActionItem, TaskToggle},
    commands_impl::Capabilities,
    query_impl::{Query, QueryResult},
    refactor::Refactor,
};
use eval::RequestContext;
use eval::Symbol;
use eval::resources::Resource;
use eval::session::WorkspaceSession;
use lsp_types::*;
use model::Problem;
use std::path::Path;
use url::Url;

use crate::inlays::InlayOutput;

/// Every editor feature, as a method on a request. `RequestContext` is
/// `eval`'s, so this is an extension trait rather than an inherent impl.
pub trait Session {
    // Presentation: what the note looks like.
    fn hints(&self, path: &Path, range: Range) -> InlayOutput;
    /// Whether this note's labels change with the clock, so hosts can tick it.
    fn live_hints(&self, path: &Path) -> bool;
    fn document_links(&self, path: &Path) -> Vec<DocumentLink>;
    fn render_text(&self, path: &Path) -> Result<String, String>;
    fn render_html(&self, path: &Path) -> Result<String, String>;

    // Problems.
    fn diagnostics(&self, path: &Path, editing: bool) -> Vec<Diagnostic>;
    /// Without the feature modules' own diagnostics, to avoid re-entering them.
    fn native_diagnostics(&self, path: &Path, editing: bool) -> Vec<Diagnostic>;
    /// Errors only, as source spans: warnings such as unfetched data are states.
    fn problems(&self, path: &Path) -> Vec<Problem>;

    // Interaction: the controls a note offers.
    fn code_lenses(&self, path: &Path, capabilities: Capabilities) -> Vec<CodeLens>;
    fn row_commands(
        &self,
        path: &Path,
        row: usize,
        include_task: bool,
        capabilities: Capabilities,
    ) -> Vec<Command>;
    fn resources_at(&self, path: &Path, row: usize) -> Vec<Resource>;
    fn code_actions(
        &self,
        path: &Path,
        range: Range,
        capabilities: Capabilities,
        toggle: TaskToggle,
    ) -> Vec<CodeActionItem>;
    fn refactors(&self, path: &Path, range: Range) -> Vec<Refactor>;
    fn toggle_task(&self, path: &Path, index: usize) -> Result<Vec<TextEdit>, String>;
    fn freeze_dates(&self, path: &Path) -> Vec<TextEdit>;
    fn formatting(&self, path: &Path) -> Result<Vec<TextEdit>, String>;
    /// Only the hovers feature modules contribute, ahead of the built-in chain.
    fn module_hover(&self, path: &Path, position: Position) -> Option<Hover>;
    /// Only the diagnostics feature modules contribute.
    fn module_diagnostics(&self, path: &Path) -> Vec<Diagnostic>;

    // Intelligence: what the editor explains.
    /// The whole hover chain, in the order a reader expects: a feature module's
    /// own hovers, then links, table cells, bracketed calculations, symbols and
    /// finally the task on this line.
    fn hover(&self, path: &Path, position: Position) -> Option<Hover>;
    fn symbol_hover(&self, symbol: &Symbol) -> String;
    fn link_hover(&self, path: &Path, position: Position) -> Option<Hover>;
    fn cell_hover(&self, path: &Path, position: Position) -> Option<Hover>;
    fn calculation_hover(&self, path: &Path, position: Position) -> Option<Hover>;
    fn completions(&self, path: &Path, position: Position, snippets: bool) -> Vec<CompletionItem>;

    // Navigation.
    fn document_symbols(&self, path: &Path) -> Vec<DocumentSymbol>;
    fn hierarchy_item(&self, symbol: &Symbol) -> CallHierarchyItem;

    // The workspace query API, optionally scoped to one indexed note.
    fn query(&self, query: &Query, only: Option<&Path>) -> Result<QueryResult, String>;
}
impl Session for RequestContext<'_> {
    fn hints(&self, path: &Path, range: Range) -> InlayOutput {
        crate::presentation_impl::hints(self, path, range)
    }
    fn live_hints(&self, path: &Path) -> bool {
        crate::presentation_impl::live_hints(self, path)
    }
    fn document_links(&self, path: &Path) -> Vec<DocumentLink> {
        crate::presentation_impl::document_links(self, path)
    }
    fn render_text(&self, path: &Path) -> Result<String, String> {
        crate::presentation_impl::rendered_text(self, path)
    }
    fn render_html(&self, path: &Path) -> Result<String, String> {
        crate::rendering_impl::html_for(self, path)
    }
    fn diagnostics(&self, path: &Path, editing: bool) -> Vec<Diagnostic> {
        crate::diagnostics_impl::collect(self, path, editing)
    }
    fn native_diagnostics(&self, path: &Path, editing: bool) -> Vec<Diagnostic> {
        crate::diagnostics_impl::collect_native(self, path, editing)
    }
    fn problems(&self, path: &Path) -> Vec<Problem> {
        crate::diagnostics_impl::problems(self, path)
    }
    fn code_lenses(&self, path: &Path, capabilities: Capabilities) -> Vec<CodeLens> {
        crate::interaction::lenses(self, path, capabilities)
    }
    fn row_commands(
        &self,
        path: &Path,
        row: usize,
        include_task: bool,
        capabilities: Capabilities,
    ) -> Vec<Command> {
        crate::interaction::row_commands(self, path, row, include_task, capabilities)
    }
    fn resources_at(&self, path: &Path, row: usize) -> Vec<Resource> {
        crate::interaction::resources_at(self, path, row)
    }
    fn code_actions(
        &self,
        path: &Path,
        range: Range,
        capabilities: Capabilities,
        toggle: TaskToggle,
    ) -> Vec<CodeActionItem> {
        crate::actions_impl::code_actions(self, path, range, capabilities, toggle)
    }
    fn refactors(&self, path: &Path, range: Range) -> Vec<Refactor> {
        crate::refactor::refactors(self, path, range)
    }
    fn toggle_task(&self, path: &Path, index: usize) -> Result<Vec<TextEdit>, String> {
        crate::actions_impl::toggle_task(self, path, index)
    }
    fn freeze_dates(&self, path: &Path) -> Vec<TextEdit> {
        crate::actions_impl::freeze_dates(self, path)
    }
    fn formatting(&self, path: &Path) -> Result<Vec<TextEdit>, String> {
        crate::modules::formatting(self, path)
    }
    fn module_hover(&self, path: &Path, position: Position) -> Option<Hover> {
        crate::modules::hover(self, path, position)
    }
    fn module_diagnostics(&self, path: &Path) -> Vec<Diagnostic> {
        crate::modules::diagnostics(self, path)
    }
    fn hover(&self, path: &Path, position: Position) -> Option<Hover> {
        crate::intelligence_impl::hover_at(self, path, position)
    }
    fn symbol_hover(&self, symbol: &Symbol) -> String {
        crate::intelligence_impl::symbol_hover(self, symbol)
    }
    fn link_hover(&self, path: &Path, position: Position) -> Option<Hover> {
        crate::intelligence_impl::link_hover(self, path, position)
    }
    fn cell_hover(&self, path: &Path, position: Position) -> Option<Hover> {
        crate::intelligence_impl::cell_hover(self, path, position)
    }
    fn calculation_hover(&self, path: &Path, position: Position) -> Option<Hover> {
        crate::intelligence_impl::calculation_hover(self, path, position)
    }
    fn completions(&self, path: &Path, position: Position, snippets: bool) -> Vec<CompletionItem> {
        crate::intelligence_impl::completions(self, path, position, snippets)
    }
    fn document_symbols(&self, path: &Path) -> Vec<DocumentSymbol> {
        crate::symbols_impl::document_symbols(self, path)
    }
    fn hierarchy_item(&self, symbol: &Symbol) -> CallHierarchyItem {
        crate::hierarchy_impl::item(self, symbol)
    }
    fn query(&self, query: &Query, only: Option<&Path>) -> Result<QueryResult, String> {
        crate::query_impl::execute(self, query, only)
    }
}

/// What one refresh found: the diagnostics a client has not been told about,
/// whether any code lens changed, and the notes whose labels still move with
/// the clock, so a host knows whether to keep ticking.
pub struct RefreshReport {
    pub diagnostics: Vec<(Url, i32, Vec<Diagnostic>)>,
    pub lenses_changed: bool,
    pub live: std::collections::BTreeSet<std::path::PathBuf>,
}

/// An open note's path and version with its freshly computed diagnostics and lenses.
type NoteUpdate = (std::path::PathBuf, i32, Vec<Diagnostic>, Vec<CodeLens>);

/// One evaluation of the open notes: their diagnostics, their lenses, and which
/// of them read the clock.
fn evaluate(
    session: &WorkspaceSession,
    now: chrono::DateTime<chrono::FixedOffset>,
    paths: Vec<(std::path::PathBuf, i32)>,
) -> (
    std::collections::BTreeSet<std::path::PathBuf>,
    Vec<NoteUpdate>,
) {
    let request = RequestContext::new(&session.workspace, now);
    let live = paths
        .iter()
        .filter(|(path, _)| request.live_hints(path))
        .map(|(path, _)| path.clone())
        .collect();
    let updates = paths
        .into_iter()
        .map(|(path, version)| {
            let diagnostics = request.diagnostics(&path, true);
            let lenses = request.code_lenses(&path, Capabilities::NATIVE);
            (path, version, diagnostics, lenses)
        })
        .collect();
    (live, updates)
}

/// `WorkspaceSession` is `eval`'s, so refreshing it is an extension trait too.
pub trait SessionRefresh {
    /// Re-evaluate every open note and report all of it: what a host does once
    /// the workspace itself has changed, so nothing cached can be trusted.
    fn refresh(&mut self, now: chrono::DateTime<chrono::FixedOffset>) -> RefreshReport;
    /// Re-evaluate only the notes whose labels move with the clock, and report
    /// what actually changed: a tick that finds nothing says nothing. `None`
    /// when no note is live, so an idle host does no work at all.
    fn refresh_live(&mut self, now: chrono::DateTime<chrono::FixedOffset>)
    -> Option<RefreshReport>;
}
impl SessionRefresh for WorkspaceSession {
    fn refresh(&mut self, now: chrono::DateTime<chrono::FixedOffset>) -> RefreshReport {
        let paths = refreshable(self, |_| true);
        let (live, updates) = evaluate(self, now, paths);
        self.live = live.clone();
        self.diagnostics.clear();
        self.lenses.clear();
        let mut diagnostics = Vec::new();
        for (path, version, ds, lenses) in updates {
            diagnostics.push((common::uri(&path), version, ds.clone()));
            self.diagnostics.insert(path.clone(), ds);
            self.lenses.insert(path, lenses);
        }
        RefreshReport {
            diagnostics,
            lenses_changed: true,
            live,
        }
    }

    fn refresh_live(
        &mut self,
        now: chrono::DateTime<chrono::FixedOffset>,
    ) -> Option<RefreshReport> {
        if self.live().is_empty() {
            return None;
        }
        let paths = refreshable(self, |path| self.live().contains(path));
        let (live, updates) = evaluate(self, now, paths);
        let mut diagnostics = Vec::new();
        let mut lenses_changed = false;
        for (path, version, ds, lenses) in updates {
            if self.diagnostics.get(&path) != Some(&ds) {
                diagnostics.push((common::uri(&path), version, ds.clone()));
                self.diagnostics.insert(path.clone(), ds);
            }
            if self.lenses.get(&path) != Some(&lenses) {
                self.lenses.insert(path, lenses);
                lenses_changed = true;
            }
        }
        self.live = live.clone();
        Some(RefreshReport {
            diagnostics,
            lenses_changed,
            live,
        })
    }
}

/// The open notes that are in the index, and so can be evaluated at all.
fn refreshable(
    session: &WorkspaceSession,
    keep: impl Fn(&Path) -> bool,
) -> Vec<(std::path::PathBuf, i32)> {
    session
        .versions()
        .iter()
        .filter(|(path, _)| session.workspace.documents.contains_key(*path) && keep(path))
        .map(|(path, version)| (path.clone(), *version))
        .collect()
}
