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
//! A session also keeps what requests derive from its workspace ([`Shared`]):
//! the records, the evaluator's results and feature modules'
//! answers, so the requests between two changes share one build of each
//! collection and one evaluation of each definition and hook, for as long as
//! the clock they read allows. The workspace is reached only through
//! [`WorkspaceSession::workspace`] and [`WorkspaceSession::workspace_mut`], and
//! every change starts all of it over.
use crate::{Request, commands::Capabilities, request::Shared};
use chrono::{DateTime, FixedOffset};
use lang::document::Document;
use lang::eval::Workspace;
use lsp_types::{
    CodeLens, Diagnostic, DocumentChanges, OneOf, OptionalVersionedTextDocumentIdentifier,
    TextDocumentEdit, TextEdit, WorkspaceEdit,
};
use records::{NoteFiles, Query};
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
    shared: Shared,
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
            shared: Shared::default(),
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
        std::mem::take(&mut self.shared).retire();
        &mut self.workspace
    }
    /// A request over the workspace at `now`, sharing what earlier requests
    /// derived from this revision of it.
    pub fn request(&self, now: DateTime<FixedOffset>) -> Request<'_> {
        Request::sharing(&self.workspace, now, &self.shared)
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
            std::mem::take(&mut self.shared).retire();
            let document = self.workspace.parse(text);
            self.workspace.insert_document(path.to_path_buf(), document);
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
        std::mem::take(&mut self.shared).retire();
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
    /// `changes` as one workspace edit, each note at the version it is open at.
    pub fn edit(
        &self,
        changes: impl IntoIterator<Item = (PathBuf, Vec<TextEdit>)>,
    ) -> WorkspaceEdit {
        let changes = changes.into_iter().map(|(path, edits)| TextDocumentEdit {
            text_document: OptionalVersionedTextDocumentIdentifier {
                uri: lang::common::uri_from_url(&lang::common::uri(&path)),
                version: self.version(&path),
            },
            edits: edits.into_iter().map(OneOf::Left).collect(),
        });
        WorkspaceEdit {
            document_changes: Some(DocumentChanges::Edits(changes.collect())),
            ..Default::default()
        }
    }
    /// `query` over the workspace at `now`, optionally scoped to one note, as
    /// a host answers it: the rows, with the open versions they were read at.
    pub fn query(
        &self,
        query: &Query,
        only: Option<&Path>,
        now: DateTime<FixedOffset>,
    ) -> Result<Value, String> {
        let rows = self.request(now).query(query, only)?.json();
        Ok(
            json!({"schemaVersion":1,"now":now.to_rfc3339(),"rows":rows,"versions":self.versions_json()}),
        )
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
/// and whether any code lens changed.
pub struct RefreshReport {
    pub diagnostics: Vec<(Url, i32, Vec<Diagnostic>)>,
    pub lenses_changed: bool,
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
        let request = Request::sharing(&self.workspace, now, &self.shared);
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
            .collect();
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
        self.live = live;
        RefreshReport {
            diagnostics,
            lenses_changed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lang::eval::engine::{Measured, Value as Item};
    use lsp_types::{Position, Range};

    /// A list value, shared by every request that reuses it.
    type Items = Arc<Measured<Vec<Item>>>;

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
        let path = Path::new(NOTE);
        let text = "stamp := now()\n- [ ] Water :water @every(day)\n";
        let mut session = open(text);
        let due = "map(tasks, fn(t) => t.due)";
        let stamp = "map(values, fn(v) => v.display)";

        assert_eq!(rows(&session, MORNING, due), rows(&session, NOON, due));
        assert_ne!(rows(&session, MORNING, due), rows(&session, TOMORROW, due));
        assert_ne!(rows(&session, MORNING, stamp), rows(&session, NOON, stamp));

        let titles = "map(tasks, fn(t) => t.title)";
        assert_eq!(rows(&session, MORNING, titles), json!(["Water"]));
        session
            .open(path, 2, text.replace("Water", "Feed"))
            .unwrap();
        assert_eq!(rows(&session, MORNING, titles), json!(["Feed"]));
        session.workspace_mut().remove_document(path);
        assert_eq!(rows(&session, MORNING, titles), json!([]));
    }

    const NOTE: &str = "/workspace/note.x.md";
    const MORNING: &str = "2026-09-16T09:00:00-04:00";
    const NOON: &str = "2026-09-16T12:00:00-04:00";
    const TOMORROW: &str = "2026-09-17T09:00:00-04:00";

    fn open(text: &str) -> WorkspaceSession {
        let mut session = WorkspaceSession::browser(Workspace::new(vec!["/workspace".into()]));
        session.open(Path::new(NOTE), 1, text.into()).unwrap();
        session
    }
    /// The list a read gave.
    fn items(read: Result<Item, impl std::fmt::Debug>) -> Items {
        match read {
            Ok(Item::List(items)) => items,
            other => panic!("expected a list, found {other:?}"),
        }
    }
    /// The list `name` evaluates to in a request at `now`: the very value an
    /// earlier request evaluated, when the session reuses it.
    fn list(session: &WorkspaceSession, now: &str, name: &str) -> Items {
        let request = session.request(at(now));
        items(request.engine().named(Path::new(NOTE), name))
    }
    fn quote(session: &mut WorkspaceSession, price: f64) {
        let key = "quote:NVDA".to_string();
        let fetched = at(MORNING).to_utc();
        let lookup = lang::eval::lookups::Lookup::new(
            json!({"price": price, "currency": "USD"}),
            fetched,
            "test".into(),
        );
        session.workspace_mut().store_lookup(key, lookup);
    }

    /// Requests share evaluated values while the workspace stands and the
    /// clock reads the same to them; an edit, a clock tick a value reads, a
    /// lookup refresh or a module change starts them over.
    #[test]
    fn evaluations_are_shared_until_the_workspace_or_their_clock_moves() {
        let text = "steady := [1, 2]\nstamp := [now()]\nprice := [quote(NVDA)]\n";
        let mut session = open(text);
        quote(&mut session, 1.0);
        let same = Arc::ptr_eq;
        let steady = list(&session, MORNING, "steady");
        assert!(same(&steady, &list(&session, MORNING, "steady")));
        assert!(same(&steady, &list(&session, NOON, "steady")));
        assert!(!same(&steady, &list(&session, TOMORROW, "steady")));
        // A tick: what reads `now()` is evaluated again, at the new instant.
        let stamp = list(&session, MORNING, "stamp");
        assert!(same(&stamp, &list(&session, MORNING, "stamp")));
        assert_ne!(stamp, list(&session, NOON, "stamp"));

        let steady = list(&session, MORNING, "steady");
        session.open(Path::new(NOTE), 2, text.into()).unwrap();
        assert!(!same(&steady, &list(&session, MORNING, "steady")));

        let price = list(&session, MORNING, "price");
        assert!(same(&price, &list(&session, MORNING, "price")));
        quote(&mut session, 2.0);
        assert_ne!(price, list(&session, MORNING, "price"));

        let steady = list(&session, MORNING, "steady");
        let modules = session.workspace().modules().clone();
        session.workspace_mut().replace_modules(modules);
        assert!(!same(&steady, &list(&session, MORNING, "steady")));
    }

    /// The records' hovers, as a feature module reading them sees them.
    fn hovers(session: &WorkspaceSession, now: &str) -> Items {
        let request = session.request(at(now));
        let fields = ["hover".to_owned()];
        let view = records::View::Fields(&fields);
        let collection = lang::eval::modules::Collection::Values;
        let path = Some(Path::new(NOTE));
        let read = request
            .records
            .view(&mut request.engine(), path, collection, view, |_, _| {
                Vec::new()
            });
        items(read)
    }

    /// A hover that reads the clock only through the date is kept all day,
    /// like the rest of its records; one that reads it more finely — a value
    /// from `now()`, a lookup's age — only at that instant.
    #[test]
    fn hovers_are_kept_for_as_long_as_the_clock_they_read() {
        let session = open("total := 2 + 3\ndue := today()\n");
        let morning = hovers(&session, MORNING);
        assert!(Arc::ptr_eq(&morning, &hovers(&session, NOON)));
        assert!(!Arc::ptr_eq(&morning, &hovers(&session, TOMORROW)));

        let stamped = open("stamp := now()\n");
        assert_ne!(hovers(&stamped, MORNING), hovers(&stamped, NOON));
        let mut aged = open("price := quote(NVDA)\n");
        quote(&mut aged, 1.0);
        let morning = hovers(&aged, MORNING);
        assert!(Arc::ptr_eq(&morning, &hovers(&aged, MORNING)));
        assert_ne!(morning, hovers(&aged, NOON));
    }

    /// A feature module's hook answers again only when its input or the
    /// clock it read has moved: a request later in the day reuses every
    /// answer, and an edit starts them over.
    #[test]
    fn hook_answers_are_kept_while_their_input_and_clock_hold() {
        let text = "total := 2 + 3\n\nWe have [total].\n\n- [ ] Water @due(2026-09-20)\n";
        let mut session = open(text);
        let range = Range::new(Position::new(0, 0), Position::new(5, 0));
        let hints = |session: &WorkspaceSession, now: &str| {
            let request = session.request(at(now));
            let hints = request.hints(Path::new(NOTE), range);
            (json!(hints.hints), request.answers.clocks())
        };
        let (morning, clocks) = hints(&session, MORNING);
        assert!(!clocks.is_empty());
        assert!(clocks.iter().all(|clock| *clock == at(MORNING)));
        let (noon, clocks) = hints(&session, NOON);
        assert_eq!(noon, morning);
        assert!(clocks.iter().all(|clock| *clock == at(MORNING)));
        let (_, clocks) = hints(&session, TOMORROW);
        assert!(clocks.iter().all(|clock| *clock == at(TOMORROW)));

        session.open(Path::new(NOTE), 2, text.into()).unwrap();
        let (again, clocks) = hints(&session, NOON);
        assert_eq!(again, morning);
        assert!(clocks.iter().all(|clock| *clock == at(NOON)));
    }
}
