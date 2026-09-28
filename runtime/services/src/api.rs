//! The feature API and the editing session. Every editor feature is a method
//! on one [`Request`], so a host only ever locks its state, builds a request,
//! calls one method and converts the result to its transport. The
//! implementations stay in the feature module each belongs to; the `impl`
//! below is the list of what exists.
//!
//! [`WorkspaceSession`] is what a host knows about a workspace *being edited*:
//! which notes are open at which version, the unsaved buffers that shadow the
//! files on disk, and the last diagnostics, lenses and live set a refresh
//! produced. Both hosts drive the same state machine. The language server
//! keeps buffers for module sources too — a module is highlighted while it is
//! being written without being activated — and accepts a repeated version,
//! because an editor resends one when a note is reopened. The browser has
//! neither: every write must carry a strictly newer version, and a `.x.md` file
//! under the virtual workspace is always an ordinary note.
use crate::{
    controls::code_actions::{CodeActionItem, TaskToggle},
    controls::commands::Capabilities,
    data::query::{Query, QueryResult},
    view::inlays::InlayOutput,
};
use chrono::{DateTime, FixedOffset};
use lang::eval::{RequestContext, Symbol, Workspace};
use lang::model::Document;
use lsp_types::*;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
use url::Url;

/// One workspace snapshot and clock, with every editor feature as a method.
/// It derefs to `eval`'s [`RequestContext`], which the features evaluate with.
#[derive(Clone)]
pub struct Request<'a>(RequestContext<'a>);
impl<'a> Request<'a> {
    pub fn new(workspace: &'a Workspace, now: DateTime<FixedOffset>) -> Self {
        Self(RequestContext::new(workspace, now))
    }
}
impl<'a> From<RequestContext<'a>> for Request<'a> {
    fn from(context: RequestContext<'a>) -> Self {
        Self(context)
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
        crate::view::presentation::hints(self, path, range)
    }
    /// Whether this note's labels change with the clock, so hosts can tick it.
    pub fn live_hints(&self, path: &Path) -> bool {
        crate::view::presentation::live_hints(self, path)
    }
    pub fn document_links(&self, path: &Path) -> Vec<DocumentLink> {
        crate::view::presentation::document_links(self, path)
    }
    pub fn render_text(&self, path: &Path) -> Result<String, String> {
        crate::view::presentation::rendered_text(self, path)
    }
    pub fn render_html(&self, path: &Path) -> Result<String, String> {
        crate::view::rendering::html_for(self, path)
    }

    // Problems.
    pub fn diagnostics(&self, path: &Path, editing: bool) -> Vec<Diagnostic> {
        crate::providers::diagnostics(self, path, editing)
    }

    // Interaction: the controls a note offers.
    pub fn code_lenses(&self, path: &Path, capabilities: Capabilities) -> Vec<CodeLens> {
        crate::controls::rows::lenses(self, path, capabilities)
    }
    pub fn code_actions(
        &self,
        path: &Path,
        range: Range,
        capabilities: Capabilities,
        toggle: TaskToggle,
    ) -> Vec<CodeActionItem> {
        crate::controls::code_actions::code_actions(self, path, range, capabilities, toggle)
    }
    pub fn formatting(&self, path: &Path) -> Result<Vec<TextEdit>, String> {
        crate::providers::edits(self, path)
    }

    // Intelligence: what the editor explains.
    /// The whole hover chain, in the order a reader expects: a feature module's
    /// own hovers, then links, table cells, bracketed calculations, symbols and
    /// finally the task on this line.
    pub fn hover(&self, path: &Path, position: Position) -> Option<Hover> {
        crate::providers::hover(self, path, position)
    }
    pub fn completions(
        &self,
        path: &Path,
        position: Position,
        snippets: bool,
    ) -> Vec<CompletionItem> {
        crate::language::completion::completions(self, path, position, snippets)
    }

    // Navigation.
    pub fn document_symbols(&self, path: &Path) -> Vec<DocumentSymbol> {
        crate::language::symbols::document_symbols(self, path)
    }
    pub fn hierarchy_item(&self, symbol: &Symbol) -> CallHierarchyItem {
        crate::language::hierarchy::item(self, symbol)
    }

    // The workspace query API, optionally scoped to one indexed note.
    pub fn query(&self, query: &Query, only: Option<&Path>) -> Result<QueryResult, String> {
        crate::data::query::execute(self, query, only)
    }
}

/// Which host is driving the session, and therefore how repeated versions and
/// module sources are treated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Editor,
    Browser,
}

/// Reading the notes a workspace imports from disk. `services` does no I/O:
/// a native host supplies this (`host::DiskFiles`), and a browser has none.
pub trait NoteFiles: Send + Sync {
    /// Follow explicit imports, reading every imported note not yet loaded.
    fn load_imports(&self, workspace: &mut Workspace);
    /// Read one note, and what it imports, unless it is already loaded.
    fn include_file(&self, workspace: &mut Workspace, path: &Path) -> Result<(), String>;
}

pub struct WorkspaceSession {
    pub workspace: Workspace,
    kind: Kind,
    /// How an editor follows imports from disk; a browser has no files.
    files: Option<std::sync::Arc<dyn NoteFiles>>,
    open: BTreeMap<PathBuf, i32>,
    module_buffers: BTreeMap<PathBuf, Document>,
    pub live: BTreeSet<PathBuf>,
    pub diagnostics: BTreeMap<PathBuf, Vec<Diagnostic>>,
    pub lenses: BTreeMap<PathBuf, Vec<CodeLens>>,
}

impl WorkspaceSession {
    /// A language-server session: module buffers, versions that may repeat,
    /// and imports followed on disk through `files`.
    pub fn editor(workspace: Workspace, files: std::sync::Arc<dyn NoteFiles>) -> Self {
        Self::with_kind(workspace, Kind::Editor, Some(files))
    }
    /// A browser session: no module buffers, and strictly increasing versions.
    pub fn browser(workspace: Workspace) -> Self {
        Self::with_kind(workspace, Kind::Browser, None)
    }
    fn with_kind(
        workspace: Workspace,
        kind: Kind,
        files: Option<std::sync::Arc<dyn NoteFiles>>,
    ) -> Self {
        Self {
            workspace,
            kind,
            files,
            open: BTreeMap::new(),
            module_buffers: BTreeMap::new(),
            live: BTreeSet::new(),
            diagnostics: BTreeMap::new(),
            lenses: BTreeMap::new(),
        }
    }

    /// Take a buffer for `path`. Module sources are parsed into a buffer of
    /// their own, so they can be highlighted without their definitions joining
    /// the note index; everything else becomes an open note, bypassing the
    /// discovery filters that hide ignored directories from a scan.
    pub fn open(&mut self, path: &Path, version: i32, text: String) -> Result<(), String> {
        if self.open.get(path).is_some_and(|old| match self.kind {
            Kind::Editor => *old > version,
            Kind::Browser => version <= *old,
        }) {
            return Err("Stale document version".into());
        }
        if self.is_module_path(path) {
            self.module_buffers
                .insert(path.to_path_buf(), Document::parse(text));
        } else {
            self.workspace
                .insert_document(path.to_path_buf(), Document::parse(text));
            if let Some(files) = &self.files {
                files.load_imports(&mut self.workspace);
            }
        }
        self.open.insert(path.to_path_buf(), version);
        Ok(())
    }

    /// Forget a buffer. The note itself is reloaded from disk by the next
    /// [`WorkspaceSession::rescan`].
    pub fn close(&mut self, path: &Path) {
        self.open.remove(path);
        self.module_buffers.remove(path);
    }

    /// Adopt a workspace freshly read from disk, re-overlaying the open buffers
    /// on top of it: a note that has become a module keeps its buffer aside,
    /// one that has stopped being a module gets its buffer back, and activated
    /// module paths leave the note index entirely.
    pub fn rescan(&mut self, mut fresh: Workspace) {
        let open: Vec<_> = self.open.keys().cloned().collect();
        for path in open {
            if module_path(&fresh, &path) {
                if let Some(doc) = self.workspace.documents().get(&path).cloned() {
                    self.module_buffers.insert(path, doc);
                }
            } else if let Some(doc) = self
                .module_buffers
                .remove(&path)
                .or_else(|| self.workspace.documents().get(&path).cloned())
            {
                fresh.insert_document(path, doc);
            }
        }
        if let Some(files) = &self.files {
            files.load_imports(&mut fresh);
        }
        let modules = fresh.modules().clone();
        fresh.retain_documents(|path| !modules.modules.iter().any(|m| m.path == path));
        self.workspace = fresh;
    }

    /// Whether this path is a module source rather than a note.
    pub fn is_module_path(&self, path: &Path) -> bool {
        self.kind == Kind::Editor && module_path(&self.workspace, path)
    }

    /// The parsed text to colour and fold: a module buffer, or the note itself.
    pub fn document_for_highlighting(&self, path: &Path) -> Option<&Document> {
        self.module_buffers
            .get(path)
            .or_else(|| self.workspace.documents().get(path))
    }

    pub fn versions(&self) -> &BTreeMap<PathBuf, i32> {
        &self.open
    }
    pub fn version(&self, path: &Path) -> Option<i32> {
        self.open.get(path).copied()
    }
    /// The open versions as a client sees them, keyed by file URI.
    pub fn versions_json(&self) -> Value {
        Value::Object(
            self.open
                .iter()
                .map(|(path, version)| (lang::common::uri(path).to_string(), json!(version)))
                .collect(),
        )
    }
    pub fn live(&self) -> &BTreeSet<PathBuf> {
        &self.live
    }
    /// Stop ticking: a shutting-down server has nothing left to refresh.
    pub fn clear_live(&mut self) {
        self.live.clear();
    }
}

/// A module source either by location or because a manifest activated it.
fn module_path(workspace: &Workspace, path: &Path) -> bool {
    lang::eval::modules::is_module_path(path)
        || workspace.modules().modules.iter().any(|m| m.path == path)
}

/// What one refresh found: the diagnostics a client has not been told about,
/// whether any code lens changed, and the notes whose labels still move with
/// the clock, so a host knows whether to keep ticking.
pub struct RefreshReport {
    pub diagnostics: Vec<(Url, i32, Vec<Diagnostic>)>,
    pub lenses_changed: bool,
    pub live: BTreeSet<PathBuf>,
}

/// An open note's path and version with its freshly computed diagnostics and lenses.
type NoteUpdate = (PathBuf, i32, Vec<Diagnostic>, Vec<CodeLens>);

/// One evaluation of the open notes: their diagnostics, their lenses, and which
/// of them read the clock.
fn evaluate(
    session: &WorkspaceSession,
    now: DateTime<FixedOffset>,
    paths: Vec<(PathBuf, i32)>,
) -> (BTreeSet<PathBuf>, Vec<NoteUpdate>) {
    let request = Request::new(&session.workspace, now);
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
    pub fn refresh(&mut self, now: DateTime<FixedOffset>) -> RefreshReport {
        let paths = refreshable(self, |_| true);
        let (live, updates) = evaluate(self, now, paths);
        self.live = live.clone();
        self.diagnostics.clear();
        self.lenses.clear();
        let mut diagnostics = Vec::new();
        for (path, version, ds, lenses) in updates {
            diagnostics.push((lang::common::uri(&path), version, ds.clone()));
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
    pub fn refresh_live(&mut self, now: DateTime<FixedOffset>) -> Option<RefreshReport> {
        if self.live().is_empty() {
            return None;
        }
        let paths = refreshable(self, |path| self.live().contains(path));
        let (live, updates) = evaluate(self, now, paths);
        let mut diagnostics = Vec::new();
        let mut lenses_changed = false;
        for (path, version, ds, lenses) in updates {
            if self.diagnostics.get(&path) != Some(&ds) {
                diagnostics.push((lang::common::uri(&path), version, ds.clone()));
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
fn refreshable(session: &WorkspaceSession, keep: impl Fn(&Path) -> bool) -> Vec<(PathBuf, i32)> {
    session
        .versions()
        .iter()
        .filter(|(path, _)| session.workspace.documents().contains_key(*path) && keep(path))
        .map(|(path, version)| (path.clone(), *version))
        .collect()
}
