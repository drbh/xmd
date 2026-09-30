//! [`WorkspaceSession`] is what a host knows about a workspace *being edited*:
//! which notes are open at which version, the unsaved buffers that shadow the
//! files on disk, and the last diagnostics, lenses and live set a refresh
//! produced. Both hosts drive the same state machine. The language server
//! keeps buffers for module sources too — a module is highlighted while it is
//! being written without being activated — and accepts a repeated version,
//! because an editor resends one when a note is reopened. The browser has
//! neither: every write must carry a strictly newer version, and a `.x.md` file
//! under the virtual workspace is always an ordinary note.
//!
//! A session also keeps the catalog's derived [`Records`] for its workspace,
//! so the requests between two changes share one build of each collection.
//! The workspace is reached only through [`WorkspaceSession::workspace`] and
//! [`WorkspaceSession::workspace_mut`], and every change starts the records
//! over.
use crate::{Request, commands::Capabilities};
use catalog::{NoteFiles, Records};
use chrono::{DateTime, FixedOffset};
use lang::eval::Workspace;
use lang::model::Document;
use lsp_types::{CodeLens, Diagnostic};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};
use url::Url;

/// Which host is driving the session, and therefore how repeated versions and
/// module sources are treated. Only an editor follows imports from disk.
enum Driver {
    Editor(Arc<dyn NoteFiles>),
    Browser,
}

pub struct WorkspaceSession {
    workspace: Workspace,
    /// Derived from `workspace` as it is now; replaced whenever it changes.
    records: Arc<Records>,
    driver: Driver,
    open: BTreeMap<PathBuf, i32>,
    module_buffers: BTreeMap<PathBuf, Document>,
    live: BTreeSet<PathBuf>,
    diagnostics: BTreeMap<PathBuf, Vec<Diagnostic>>,
    lenses: BTreeMap<PathBuf, Vec<CodeLens>>,
}

impl WorkspaceSession {
    /// A language-server session: module buffers, versions that may repeat,
    /// and imports followed on disk through `files`.
    pub fn editor(workspace: Workspace, files: Arc<dyn NoteFiles>) -> Self {
        Self::driven_by(workspace, Driver::Editor(files))
    }
    /// A browser session: no module buffers, and strictly increasing versions.
    pub fn browser(workspace: Workspace) -> Self {
        Self::driven_by(workspace, Driver::Browser)
    }
    fn driven_by(workspace: Workspace, driver: Driver) -> Self {
        Self {
            workspace,
            records: Arc::default(),
            driver,
            open: BTreeMap::new(),
            module_buffers: BTreeMap::new(),
            live: BTreeSet::new(),
            diagnostics: BTreeMap::new(),
            lenses: BTreeMap::new(),
        }
    }

    /// The workspace as the session holds it.
    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }
    /// The workspace to change. Whatever was derived from it is dropped.
    pub fn workspace_mut(&mut self) -> &mut Workspace {
        self.records = Arc::default();
        &mut self.workspace
    }
    /// A request over the workspace at `now`, sharing what earlier requests
    /// derived from this revision of it.
    pub fn request(&self, now: DateTime<FixedOffset>) -> Request<'_> {
        Request::sharing(&self.workspace, now, self.records.clone())
    }

    /// Take a buffer for `path`. Module sources are parsed into a buffer of
    /// their own, so they can be highlighted without their definitions joining
    /// the note index; everything else becomes an open note, bypassing the
    /// discovery filters that hide ignored directories from a scan.
    pub fn open(&mut self, path: &Path, version: i32, text: String) -> Result<(), String> {
        if self.open.get(path).is_some_and(|old| match self.driver {
            Driver::Editor(_) => *old > version,
            Driver::Browser => version <= *old,
        }) {
            return Err("Stale document version".into());
        }
        if self.is_module_path(path) {
            self.module_buffers
                .insert(path.to_path_buf(), Document::parse(text));
        } else {
            self.records = Arc::default();
            self.workspace
                .insert_document(path.to_path_buf(), Document::parse(text));
            if let Driver::Editor(files) = &self.driver {
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
        if let Driver::Editor(files) = &self.driver {
            files.load_imports(&mut fresh);
        }
        let modules = fresh.modules().clone();
        fresh.retain_documents(|path| !modules.iter().any(|m| m.path == path));
        self.workspace = fresh;
        self.records = Arc::default();
    }

    /// Whether this path is a module source rather than a note.
    pub(crate) fn is_module_path(&self, path: &Path) -> bool {
        matches!(self.driver, Driver::Editor(_)) && module_path(&self.workspace, path)
    }

    /// The parsed text to colour and fold: a module buffer, or the note itself.
    pub fn document_for_highlighting(&self, path: &Path) -> Option<&Document> {
        self.module_buffers
            .get(path)
            .or_else(|| self.workspace.documents().get(path))
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
    /// Stop ticking: a shutting-down server has nothing left to refresh.
    pub fn clear_live(&mut self) {
        self.live.clear();
    }
}

/// A module source either by location or because a manifest activated it.
fn module_path(workspace: &Workspace, path: &Path) -> bool {
    lang::eval::modules::is_module_path(path) || workspace.modules().iter().any(|m| m.path == path)
}

/// What one refresh found: the diagnostics a client has not been told about,
/// whether any code lens changed, and the notes whose labels still move with
/// the clock, so a host knows whether to keep ticking.
pub struct RefreshReport {
    pub diagnostics: Vec<(Url, i32, Vec<Diagnostic>)>,
    pub lenses_changed: bool,
    pub live: BTreeSet<PathBuf>,
}

impl WorkspaceSession {
    /// Re-evaluate every open note and report all of it: what a host does once
    /// the workspace itself has changed, so nothing cached can be trusted.
    pub fn refresh(&mut self, now: DateTime<FixedOffset>) -> RefreshReport {
        self.diagnostics.clear();
        self.lenses.clear();
        self.reevaluate(now, true)
    }

    /// Re-evaluate only the notes whose labels move with the clock, and report
    /// what actually changed: a tick that finds nothing says nothing. `None`
    /// when no note is live, so an idle host does no work at all.
    pub fn refresh_live(&mut self, now: DateTime<FixedOffset>) -> Option<RefreshReport> {
        (!self.live.is_empty()).then(|| self.reevaluate(now, false))
    }

    /// Evaluate the open notes in the index — all of them, or only the live
    /// ones — and report each diagnostic set and lens list that differs from
    /// the last one sent. After [`Self::refresh`] clears them, that is all.
    fn reevaluate(&mut self, now: DateTime<FixedOffset>, all: bool) -> RefreshReport {
        let request = Request::sharing(&self.workspace, now, self.records.clone());
        let paths: Vec<_> = self
            .open
            .iter()
            .filter(|(path, _)| {
                self.workspace.documents().contains_key(*path) && (all || self.live.contains(*path))
            })
            .collect();
        let live = paths
            .iter()
            .filter(|(path, _)| request.live_hints(path))
            .map(|(path, _)| (*path).clone())
            .collect::<BTreeSet<_>>();
        let mut diagnostics = Vec::new();
        let mut lenses_changed = all;
        for (path, &version) in paths {
            let found = request.diagnostics(path, true);
            if self.diagnostics.get(path) != Some(&found) {
                diagnostics.push((lang::common::uri(path), version, found.clone()));
                self.diagnostics.insert(path.clone(), found);
            }
            let lenses = request.code_lenses(path, Capabilities::NATIVE);
            if self.lenses.get(path) != Some(&lenses) {
                self.lenses.insert(path.clone(), lenses);
                lenses_changed = true;
            }
        }
        self.live = live.clone();
        RefreshReport {
            diagnostics,
            lenses_changed,
            live,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use catalog::Query;

    fn at(now: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(now).unwrap()
    }
    fn rows(session: &WorkspaceSession, now: &str, query: &str) -> Value {
        let query = Query::parse(query).unwrap();
        session.request(at(now)).query(&query, None).unwrap().json()
    }

    /// The records a session shares between requests follow every edit and
    /// every clock they depend on: the date for a repeating task, the instant
    /// for a value that reads it.
    #[test]
    fn shared_records_follow_edits_and_the_clock() {
        let path = Path::new("/workspace/note.x.md");
        let mut session = WorkspaceSession::browser(Workspace::new(vec!["/workspace".into()]));
        let text = "stamp := now()\n- [ ] Water :water @every(day)\n";
        session.open(path, 1, text.into()).unwrap();
        let due = "map(tasks, fn(t) => t.due)";
        let stamp = "map(values, fn(v) => v.display)";

        let morning = "2026-09-16T09:00:00-04:00";
        let noon = "2026-09-16T12:00:00-04:00";
        let tomorrow = "2026-09-17T09:00:00-04:00";
        assert_eq!(rows(&session, morning, due), rows(&session, noon, due));
        assert_ne!(rows(&session, morning, due), rows(&session, tomorrow, due));
        assert_ne!(rows(&session, morning, stamp), rows(&session, noon, stamp));

        let titles = "map(tasks, fn(t) => t.title)";
        assert_eq!(rows(&session, morning, titles), json!(["Water"]));
        session
            .open(path, 2, text.replace("Water", "Feed"))
            .unwrap();
        assert_eq!(rows(&session, morning, titles), json!(["Feed"]));
        session.workspace_mut().remove_document(path);
        assert_eq!(rows(&session, morning, titles), json!([]));
    }
}
