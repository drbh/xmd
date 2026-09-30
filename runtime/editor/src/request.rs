//! The feature API. Every editor feature is a method on one [`Request`], so a
//! host only ever locks its state, builds a request, calls one method and
//! converts the result to its transport. The implementations stay in the
//! feature module each belongs to; the `impl` below is the list of what exists.
use crate::{
    code_actions,
    commands::{Action, Capabilities, PreparedAction, TaskToggle},
    completion,
    inlays::InlayOutput,
    links, providers, render,
};
use analysis::{CodeActionItem, document_symbols, hierarchy};
use catalog::{Query, QueryResult, Records};
use chrono::{DateTime, FixedOffset};
use lang::eval::{RequestContext, Symbol, Workspace};
use lsp_types::*;
use std::{path::Path, sync::Arc};

/// One workspace snapshot and clock, with every editor feature as a method.
/// It derefs to `eval`'s [`RequestContext`], which the features evaluate with,
/// and carries the catalog's derived [`Records`] every feature reads records
/// from: its own, or a session's, shared by the requests of one workspace
/// revision.
pub struct Request<'a> {
    context: RequestContext<'a>,
    /// Read directly by the features, the one thing a request adds to its
    /// context.
    pub(crate) records: Arc<Records>,
}
impl<'a> Request<'a> {
    /// A request of its own: its records are built for it and dropped with it.
    pub fn new(workspace: &'a Workspace, now: DateTime<FixedOffset>) -> Self {
        Self::sharing(workspace, now, Arc::default())
    }
    /// A request reading `records`, which its owner keeps only for as long
    /// as `workspace` is unchanged.
    pub(crate) fn sharing(
        workspace: &'a Workspace,
        now: DateTime<FixedOffset>,
        records: Arc<Records>,
    ) -> Self {
        Self {
            context: RequestContext::new(workspace, now),
            records,
        }
    }
    /// A request around an evaluation context the catalog hands back, as it
    /// does a [`catalog::DiagnosticSource`].
    pub(crate) fn within(context: &RequestContext<'a>) -> Self {
        Self {
            context: context.clone(),
            records: Arc::default(),
        }
    }
}
impl<'a> std::ops::Deref for Request<'a> {
    type Target = RequestContext<'a>;
    fn deref(&self) -> &Self::Target {
        &self.context
    }
}

impl Request<'_> {
    // Presentation: what the note looks like.
    pub fn hints(&self, path: &Path, range: Range) -> InlayOutput {
        providers::hints(self, path, range)
    }
    /// Whether this note's labels change with the clock, so hosts can tick it.
    pub(crate) fn live_hints(&self, path: &Path) -> bool {
        providers::live_hints(self, path)
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
        providers::lenses(self, path, capabilities)
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
    /// Validate `action` against this snapshot and clock, for the host to
    /// carry out. The host still owns version checks, applying edits,
    /// opening URLs and refreshing data.
    pub fn prepare(
        &self,
        action: &Action,
        capabilities: Capabilities,
    ) -> Result<PreparedAction, String> {
        providers::prepare(self, action, capabilities)
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
        catalog::execute(self, self.records.clone(), query, only, |request, path| {
            providers::diagnostics(&Request::within(request), path, false)
        })
    }
}
