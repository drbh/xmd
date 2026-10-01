//! The feature API. Every editor feature is a method on one [`Request`], so a
//! host only ever locks its state, builds a request, calls one method and
//! converts the result to its transport. The implementations stay in the
//! feature module each belongs to; the `impl` below is the list of what exists.
use crate::{
    code_actions,
    commands::{Action, Capabilities, PreparedAction, RowActions},
    completion,
    inlays::InlayOutput,
    links,
    modules::Answers,
    providers, render,
};
use analysis::{CodeActionItem, hierarchy};
use catalog::{Query, QueryResult, Records};
use chrono::{DateTime, FixedOffset};
use lang::eval::{Evaluations, RequestContext, Symbol, Workspace};
use lsp_types::*;
use std::{path::Path, sync::Arc};

/// One workspace snapshot and clock, with every editor feature as a method.
/// It derefs to `eval`'s [`RequestContext`], which the features evaluate with,
/// and carries what the features read instead of deriving it again: the
/// catalog's [`Records`] and feature modules' [`Answers`]. Each is its own, or
/// a session's, shared by the requests of one workspace revision
/// ([`Shared`]).
pub struct Request<'a> {
    context: RequestContext<'a>,
    /// Read directly by the features, like `answers`: what a request adds to
    /// its context.
    pub(crate) records: Arc<Records>,
    pub(crate) answers: Arc<Answers>,
}

/// What the requests over one workspace revision share, for an owner such as
/// a session to keep until the revision changes: the catalog's records, the
/// evaluator's results and feature modules' answers. Each holds only what
/// answers exactly as deriving it again at the request's clock would.
#[derive(Default)]
pub(crate) struct Shared {
    records: Arc<Records>,
    evaluations: Evaluations,
    answers: Arc<Answers>,
}
impl Shared {
    /// Let go of what a revision derived. A large note's records and answers
    /// take a while to free, so a native host frees them off the thread that
    /// is about to derive the next revision's.
    pub(crate) fn retire(self) {
        // When no thread can be started, the closure, and with it `self`, is
        // dropped here instead.
        #[cfg(not(target_arch = "wasm32"))]
        let _ = std::thread::Builder::new()
            .name("retire".into())
            .spawn(move || drop(self));
    }
}

impl<'a> Request<'a> {
    /// A request of its own: what it derives is built for it and dropped
    /// with it.
    pub fn new(workspace: &'a Workspace, now: DateTime<FixedOffset>) -> Self {
        Self::within(&RequestContext::new(workspace, now))
    }
    /// A request reading and adding to `shared`, which its owner keeps only
    /// for as long as `workspace` is unchanged.
    pub(crate) fn sharing(
        workspace: &'a Workspace,
        now: DateTime<FixedOffset>,
        shared: &Shared,
    ) -> Self {
        Self {
            context: RequestContext::sharing(workspace, now, &shared.evaluations),
            records: shared.records.clone(),
            answers: shared.answers.clone(),
        }
    }
    /// A request around an evaluation context the catalog hands back, as it
    /// does a [`catalog::DiagnosticSource`].
    pub(crate) fn within(context: &RequestContext<'a>) -> Self {
        Self {
            context: context.clone(),
            records: Arc::default(),
            answers: Arc::default(),
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
        row_actions: RowActions,
    ) -> Vec<CodeActionItem> {
        code_actions::code_actions(self, path, range, capabilities, row_actions)
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
        providers::edits(self, path, None)
    }
    /// Formatting as the note is typed: `ch` was just inserted, and
    /// `position` is the cursor after it.
    pub fn on_type(&self, path: &Path, position: Position, ch: &str) -> Vec<TextEdit> {
        crate::typing::on_type(self, path, position, ch)
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
        providers::symbols(self, path)
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
