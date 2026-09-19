//! What a host knows about a workspace *being edited*: which notes are open at
//! which version, the unsaved buffers that shadow the files on disk, and the
//! last diagnostics, lenses and live set a refresh produced.
//!
//! Both hosts drive the same state machine. The language server keeps buffers
//! for module sources too — a module is highlighted while it is being written
//! without being activated — and accepts a repeated version, because an editor
//! resends one when a note is reopened. The browser has neither: every write
//! must carry a strictly newer version, and a `.wtf` file under the virtual
//! workspace is always an ordinary note.
use crate::{document::Document, workspace::Workspace};
use lsp_types::{CodeLens, Diagnostic};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// Which host is driving the session, and therefore how repeated versions and
/// module sources are treated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Editor,
    Browser,
}

pub struct WorkspaceSession {
    pub workspace: Workspace,
    kind: Kind,
    open: BTreeMap<PathBuf, i32>,
    module_buffers: BTreeMap<PathBuf, Document>,
    pub(crate) live: BTreeSet<PathBuf>,
    pub(crate) diagnostics: BTreeMap<PathBuf, Vec<Diagnostic>>,
    pub(crate) lenses: BTreeMap<PathBuf, Vec<CodeLens>>,
}

impl WorkspaceSession {
    /// A language-server session: module buffers, and versions that may repeat.
    pub fn editor(workspace: Workspace) -> Self {
        Self::with_kind(workspace, Kind::Editor)
    }
    /// A browser session: no module buffers, and strictly increasing versions.
    pub fn browser(workspace: Workspace) -> Self {
        Self::with_kind(workspace, Kind::Browser)
    }
    fn with_kind(workspace: Workspace, kind: Kind) -> Self {
        Self {
            workspace,
            kind,
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
                .documents
                .insert(path.to_path_buf(), Document::parse(text));
            #[cfg(feature = "native")]
            if self.kind == Kind::Editor {
                self.workspace.load_imports();
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
    #[cfg(feature = "native")]
    pub fn rescan(&mut self, mut fresh: Workspace) {
        let open: Vec<_> = self.open.keys().cloned().collect();
        for path in open {
            if module_path(&fresh, &path) {
                if let Some(doc) = self.workspace.documents.get(&path).cloned() {
                    self.module_buffers.insert(path, doc);
                }
            } else if let Some(doc) = self
                .module_buffers
                .remove(&path)
                .or_else(|| self.workspace.documents.get(&path).cloned())
            {
                fresh.documents.insert(path, doc);
            }
        }
        fresh.load_imports();
        fresh
            .documents
            .retain(|path, _| !fresh.modules.modules.iter().any(|m| m.path == *path));
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
            .or_else(|| self.workspace.documents.get(path))
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
                .map(|(path, version)| (crate::paths::uri(path).to_string(), json!(version)))
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
    crate::modules::is_module_path(path) || workspace.modules.modules.iter().any(|m| m.path == path)
}
