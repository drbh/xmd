//! The feature API. Every editor feature is a method on one [`Request`], so a
//! host only ever locks its state, builds a request, calls one method and
//! converts the result to its transport. The implementations stay in the
//! feature module each belongs to; the `impl` below is the list of what exists.
use crate::{
    code_actions::{self, TaskToggle},
    commands::Capabilities,
    completion,
    inlays::{self, InlayOutput},
    links, providers, render, rows,
};
use analysis::{CodeActionItem, document_symbols, hierarchy};
use catalog::{Query, QueryResult};
use chrono::{DateTime, FixedOffset};
use lang::eval::{RequestContext, Symbol, Workspace};
use lsp_types::*;
use std::path::Path;

/// One workspace snapshot and clock, with every editor feature as a method.
/// It derefs to `eval`'s [`RequestContext`], which the features evaluate with.
pub struct Request<'a>(RequestContext<'a>);
impl<'a> Request<'a> {
    pub fn new(workspace: &'a Workspace, now: DateTime<FixedOffset>) -> Self {
        Self(RequestContext::new(workspace, now))
    }
}
impl<'a> std::ops::Deref for Request<'a> {
    type Target = RequestContext<'a>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Request<'_> {
    // Presentation: what the note looks like.
    pub fn hints(&self, path: &Path, range: Range) -> InlayOutput {
        inlays::collect(self, path, range)
    }
    /// Whether this note's labels change with the clock, so hosts can tick it.
    pub fn live_hints(&self, path: &Path) -> bool {
        inlays::live(self, path)
    }
    pub fn document_links(&self, path: &Path) -> Vec<DocumentLink> {
        links::document_links(self, path)
    }
    pub fn render_text(&self, path: &Path) -> Result<String, String> {
        render::rendered_text(self, path)
    }
    pub fn render_html(&self, path: &Path) -> Result<String, String> {
        render::html_for(self, path)
    }

    // Problems.
    pub fn diagnostics(&self, path: &Path, editing: bool) -> Vec<Diagnostic> {
        providers::diagnostics(self, path, editing)
    }

    // Interaction: the controls a note offers.
    pub fn code_lenses(&self, path: &Path, capabilities: Capabilities) -> Vec<CodeLens> {
        rows::lenses(self, path, capabilities)
    }
    pub fn code_actions(
        &self,
        path: &Path,
        range: Range,
        capabilities: Capabilities,
        toggle: TaskToggle,
    ) -> Vec<CodeActionItem> {
        code_actions::code_actions(self, path, range, capabilities, toggle)
    }
    pub fn formatting(&self, path: &Path) -> Result<Vec<TextEdit>, String> {
        providers::edits(self, path)
    }

    // Intelligence: what the editor explains.
    /// The whole hover chain, in the order a reader expects: a feature module's
    /// own hovers, then links, table cells, bracketed calculations, symbols and
    /// finally the task on this line.
    pub fn hover(&self, path: &Path, position: Position) -> Option<Hover> {
        providers::hover(self, path, position)
    }
    pub fn completions(
        &self,
        path: &Path,
        position: Position,
        snippets: bool,
    ) -> Vec<CompletionItem> {
        completion::completions(self, path, position, snippets)
    }

    // Navigation.
    pub fn document_symbols(&self, path: &Path) -> Vec<DocumentSymbol> {
        document_symbols(self, path)
    }
    pub fn hierarchy_item(&self, symbol: &Symbol) -> CallHierarchyItem {
        hierarchy::item(self, symbol)
    }

    // The workspace query API, optionally scoped to one indexed note.
    pub fn query(&self, query: &Query, only: Option<&Path>) -> Result<QueryResult, String> {
        catalog::execute(self, query, only, |request, path| {
            providers::diagnostics(request, path, false)
        })
    }
}
