//! The feature API. Every editor feature is a method on one request, so a host
//! only ever locks its state, builds a `RequestContext`, calls one method and
//! converts the result to its transport. The implementations stay in the
//! feature module each belongs to; this file is the list of what exists.
use crate::{
    RequestContext,
    actions::{CodeActionItem, TaskToggle},
    commands::Capabilities,
    document::Problem,
    inlays::InlayOutput,
    model::session::WorkspaceSession,
    query::{Query, QueryResult},
    refactor::Refactor,
    resources::Resource,
    workspace::Symbol,
};
use lsp_types::*;
use std::path::Path;

impl RequestContext<'_> {
    // Presentation: what the note looks like.
    pub fn hints(&self, path: &Path, range: Range) -> InlayOutput {
        crate::presentation::hints(self, path, range)
    }
    /// Whether this note's labels change with the clock, so hosts can tick it.
    pub fn live_hints(&self, path: &Path) -> bool {
        crate::presentation::live_hints(self, path)
    }
    pub fn document_links(&self, path: &Path) -> Vec<DocumentLink> {
        crate::presentation::document_links(self, path)
    }
    pub fn render_text(&self, path: &Path) -> Result<String, String> {
        crate::presentation::rendered_text(self, path)
    }
    pub fn render_html(&self, path: &Path) -> Result<String, String> {
        crate::rendering::html_for(self, path)
    }

    // Problems.
    pub fn diagnostics(&self, path: &Path, editing: bool) -> Vec<Diagnostic> {
        crate::diagnostics::collect(self, path, editing)
    }
    /// Without the feature modules' own diagnostics, to avoid re-entering them.
    pub fn native_diagnostics(&self, path: &Path, editing: bool) -> Vec<Diagnostic> {
        crate::diagnostics::collect_native(self, path, editing)
    }
    /// Errors only, as source spans: warnings such as unfetched data are states.
    pub fn problems(&self, path: &Path) -> Vec<Problem> {
        crate::diagnostics::problems(self, path)
    }

    // Interaction: the controls a note offers.
    pub fn code_lenses(&self, path: &Path, capabilities: Capabilities) -> Vec<CodeLens> {
        crate::interaction::lenses(self, path, capabilities)
    }
    pub fn row_commands(
        &self,
        path: &Path,
        row: usize,
        include_task: bool,
        capabilities: Capabilities,
    ) -> Vec<Command> {
        crate::interaction::row_commands(self, path, row, include_task, capabilities)
    }
    pub fn resources_at(&self, path: &Path, row: usize) -> Vec<Resource> {
        crate::interaction::resources_at(self, path, row)
    }
    pub fn code_actions(
        &self,
        path: &Path,
        range: Range,
        capabilities: Capabilities,
        toggle: TaskToggle,
    ) -> Vec<CodeActionItem> {
        crate::actions::code_actions(self, path, range, capabilities, toggle)
    }
    pub fn refactors(&self, path: &Path, range: Range) -> Vec<Refactor> {
        crate::refactor::refactors(self, path, range)
    }
    pub fn toggle_task(&self, path: &Path, index: usize) -> Result<Vec<TextEdit>, String> {
        crate::actions::toggle_task(self, path, index)
    }
    pub fn freeze_dates(&self, path: &Path) -> Vec<TextEdit> {
        crate::actions::freeze_dates(self, path)
    }
    pub fn formatting(&self, path: &Path) -> Result<Vec<TextEdit>, String> {
        crate::features::modules::formatting(self, path)
    }
    /// Only the hovers feature modules contribute, ahead of the built-in chain.
    pub fn module_hover(&self, path: &Path, position: Position) -> Option<Hover> {
        crate::features::modules::hover(self, path, position)
    }
    /// Only the diagnostics feature modules contribute.
    pub fn module_diagnostics(&self, path: &Path) -> Vec<Diagnostic> {
        crate::features::modules::diagnostics(self, path)
    }

    // Intelligence: what the editor explains.
    /// The whole hover chain, in the order a reader expects: a feature module's
    /// own hovers, then links, table cells, bracketed calculations, symbols and
    /// finally the task on this line.
    pub fn hover(&self, path: &Path, position: Position) -> Option<Hover> {
        crate::intelligence::hover_at(self, path, position)
    }
    pub fn symbol_hover(&self, symbol: &Symbol) -> String {
        crate::intelligence::symbol_hover(self, symbol)
    }
    pub fn link_hover(&self, path: &Path, position: Position) -> Option<Hover> {
        crate::intelligence::link_hover(self, path, position)
    }
    pub fn cell_hover(&self, path: &Path, position: Position) -> Option<Hover> {
        crate::intelligence::cell_hover(self, path, position)
    }
    pub fn calculation_hover(&self, path: &Path, position: Position) -> Option<Hover> {
        crate::intelligence::calculation_hover(self, path, position)
    }
    pub fn completions(
        &self,
        path: &Path,
        position: Position,
        snippets: bool,
    ) -> Vec<CompletionItem> {
        crate::intelligence::completions(self, path, position, snippets)
    }

    // Navigation.
    pub fn document_symbols(&self, path: &Path) -> Vec<DocumentSymbol> {
        crate::symbols::document_symbols(self, path)
    }
    pub fn hierarchy_item(&self, symbol: &Symbol) -> CallHierarchyItem {
        crate::hierarchy::item(self, symbol)
    }

    // The workspace query API, optionally scoped to one indexed note.
    pub fn query(&self, query: &Query, only: Option<&Path>) -> Result<QueryResult, String> {
        crate::query::execute(self, query, only)
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

impl WorkspaceSession {
    /// Re-evaluate every open note and report all of it: what a host does once
    /// the workspace itself has changed, so nothing cached can be trusted.
    pub fn refresh(&mut self, now: chrono::DateTime<chrono::FixedOffset>) -> RefreshReport {
        let paths = self.refreshable(|_| true);
        let (live, updates) = evaluate(self, now, paths);
        self.live = live.clone();
        self.diagnostics.clear();
        self.lenses.clear();
        let mut diagnostics = Vec::new();
        for (path, version, ds, lenses) in updates {
            diagnostics.push((crate::paths::uri(&path), version, ds.clone()));
            self.diagnostics.insert(path.clone(), ds);
            self.lenses.insert(path, lenses);
        }
        RefreshReport {
            diagnostics,
            lenses_changed: true,
            live,
        }
    }

    /// Re-evaluate only the notes whose labels move with the clock, and report
    /// what actually changed: a tick that finds nothing says nothing. `None`
    /// when no note is live, so an idle host does no work at all.
    pub fn refresh_live(
        &mut self,
        now: chrono::DateTime<chrono::FixedOffset>,
    ) -> Option<RefreshReport> {
        if self.live().is_empty() {
            return None;
        }
        let paths = self.refreshable(|path| self.live().contains(path));
        let (live, updates) = evaluate(self, now, paths);
        let mut diagnostics = Vec::new();
        let mut lenses_changed = false;
        for (path, version, ds, lenses) in updates {
            if self.diagnostics.get(&path) != Some(&ds) {
                diagnostics.push((crate::paths::uri(&path), version, ds.clone()));
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

    /// The open notes that are in the index, and so can be evaluated at all.
    fn refreshable(&self, keep: impl Fn(&Path) -> bool) -> Vec<(std::path::PathBuf, i32)> {
        self.versions()
            .iter()
            .filter(|(path, _)| self.workspace.documents.contains_key(*path) && keep(path))
            .map(|(path, version)| (path.clone(), *version))
            .collect()
    }
}
