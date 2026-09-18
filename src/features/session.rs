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
        crate::features::module_features::formatting(self, path)
    }
    /// Only the hovers feature modules contribute, ahead of the built-in chain.
    pub fn module_hover(&self, path: &Path, position: Position) -> Option<Hover> {
        crate::features::module_features::hover(self, path, position)
    }
    /// Only the diagnostics feature modules contribute.
    pub fn module_diagnostics(&self, path: &Path) -> Vec<Diagnostic> {
        crate::features::module_features::diagnostics(self, path)
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
