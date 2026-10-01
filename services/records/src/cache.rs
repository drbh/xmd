//! [`Records`]: the derived-data layer. Every consumer of a note's records —
//! feature-module hook inputs, queries — reads them here, so a collection is
//! built once per workspace revision and clock rather than once per hook,
//! module and request.
//!
//! An entry is a collection's records for one note (or the whole workspace),
//! keyed by `(note, collection)`. What it does not key on, its owner must
//! guarantee:
//!
//! - **Workspace revision.** Records read the whole workspace (imports,
//!   modules, cached lookups), so a cache belongs to one unchanging workspace.
//!   A request that builds its own cache drops it with the request; a
//!   long-lived owner such as an editor session starts a new one whenever its
//!   workspace changes. The cache also forgets everything when it is handed a
//!   different workspace than the one it was filled from.
//! - **Clock.** An entry remembers the clock it was built at. Records that
//!   read the clock only through the date and offset (a repeating task due
//!   today, a stop's time) are reused all day; once anything in the entry
//!   reads the clock more finely — a hover included, though a hover never
//!   marks its reader — the entry holds only for that instant.
//!
//! A module's `records` build ([`crate::built`]) is kept the same way, per
//! note and module, so the collections one call built share it.
//!
//! The cache knows nothing of any collection: it is handed what builds them
//! (this crate's index, `crate::collect`, and each module's `records` build),
//! so building may read other collections back through it.
//!
//! Lazy fields stay lazy: a reader that narrows a collection to a few fields
//! evaluates only those, and what they produce is kept on the cached record
//! for the next reader.
use crate::built::Build;
use crate::{Collection, DiagnosticSource, Record};
use chrono::{DateTime, FixedOffset};
use lang::common::lock;
use lang::eval::Workspace;
use lang::eval::engine::{Engine, Value};
use lang::eval::modules::Module;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

/// How a reader wants a collection's records.
#[derive(Clone, Copy, Debug)]
pub enum View<'a> {
    /// Every field, definitions evaluated: an un-narrowed feature-module input.
    Full,
    /// Only these fields: a feature module's narrowed `inputs`.
    Fields(&'a [String]),
    /// Every field and a named record's hover: what a query binds.
    Queried,
}

type Key = (Option<PathBuf>, Collection);

/// How a collection's records are built for `only` (or every note).
pub(crate) type Collect = fn(
    &Records,
    Collection,
    &mut Engine<'_>,
    Option<&Path>,
    DiagnosticSource,
) -> Result<Vec<Record>, String>;

/// How a module's `records` hook builds for the note at a path.
pub(crate) type BuildRecords = fn(&Records, &mut Engine<'_>, &Module, &Path) -> Build;

/// Derived records for one workspace, shared by every reader of it.
pub struct Records {
    state: Mutex<State>,
    collect: Collect,
}

#[derive(Default)]
struct State {
    /// The workspace the entries were built from, by address: a guard, since
    /// the owner is what keeps it unchanged.
    workspace: usize,
    entries: BTreeMap<Key, Arc<Mutex<Option<Entry>>>>,
    /// Each module's `records` build, by note and module id.
    builds: BTreeMap<(PathBuf, String), Arc<Mutex<Option<BuildEntry>>>>,
    /// Each note's text as every hook is handed it: its URI, its text and
    /// its lines.
    notes: BTreeMap<PathBuf, Arc<NoteValues>>,
}

/// A note as a hook is handed it, made once per revision.
pub(crate) struct NoteValues {
    pub(crate) uri: String,
    pub(crate) text: Value,
    pub(crate) lines: Value,
}

/// One `records` build and the clock it answers for: all day, unless reading
/// the module's inputs read the clock more finely.
struct BuildEntry {
    now: DateTime<FixedOffset>,
    exact: bool,
    build: Arc<Build>,
}

struct Entry {
    /// The clock the entry answers for.
    now: DateTime<FixedOffset>,
    /// Whether building the records read the clock: replayed onto every
    /// reader's engine, as building them again would mark it.
    built_time_dependent: bool,
    /// Whether anything in the entry reads the clock more finely than the
    /// date, so the entry holds only at `now`. A hover that does counts,
    /// though it never marks a reader's engine.
    exact: bool,
    records: Vec<Record>,
    /// Each view already read, and whether reading it marked the reader:
    /// what a repeated reader is handed.
    views: Vec<(ViewKey, Value, bool)>,
}

/// A [`View`] an entry keeps.
#[derive(PartialEq)]
enum ViewKey {
    Full,
    Fields(Vec<String>),
    Queried,
}
impl ViewKey {
    fn of(view: View<'_>) -> Self {
        match view {
            View::Full => Self::Full,
            View::Fields(keys) => Self::Fields(keys.to_vec()),
            View::Queried => Self::Queried,
        }
    }
    fn is(&self, view: View<'_>) -> bool {
        match (self, view) {
            (Self::Full, View::Full) | (Self::Queried, View::Queried) => true,
            (Self::Fields(mine), View::Fields(keys)) => mine == keys,
            _ => false,
        }
    }
}
impl Entry {
    fn new(now: DateTime<FixedOffset>, built_time_dependent: bool, records: Vec<Record>) -> Self {
        Self {
            now,
            built_time_dependent,
            exact: built_time_dependent,
            records,
            views: Vec::new(),
        }
    }
}

/// Whether what was built at `built` still holds at `now`: at that instant
/// when it is `exact`, else all that day at the same offset.
fn holds(built: DateTime<FixedOffset>, exact: bool, now: DateTime<FixedOffset>) -> bool {
    built.offset() == now.offset()
        && if exact {
            built == now
        } else {
            built.date_naive() == now.date_naive()
        }
}

impl Records {
    /// An empty cache whose collections `collect` builds.
    pub(crate) fn new(collect: Collect) -> Self {
        Self {
            state: Mutex::default(),
            collect,
        }
    }
    /// `collection`'s records for `only` (or every note), as `view` reads them.
    /// `engine` is marked time-dependent exactly when building and reading
    /// them afresh would have marked it.
    pub fn view(
        &self,
        engine: &mut Engine<'_>,
        only: Option<&Path>,
        collection: Collection,
        view: View<'_>,
        diagnostics: DiagnosticSource,
    ) -> Result<Value, String> {
        self.reading(engine, only, collection, diagnostics, |entry, engine| {
            if let Some((_, value, marks)) = entry.views.iter().find(|(key, ..)| key.is(view)) {
                engine.mark_time_dependent(*marks);
                return Ok((value.clone(), false));
            }
            let values = entry
                .records
                .iter_mut()
                .map(|r| r.read(view, engine))
                .collect::<Result<_, String>>()?;
            let value = Value::list(values);
            let marks = engine.time_dependent();
            entry.views.push((ViewKey::of(view), value.clone(), marks));
            let hover = reads_hover(&entry.records, view)
                && entry.records.iter().any(Record::hover_reads_clock);
            Ok((value, hover))
        })
    }

    /// Run `read` over the entry for `(only, collection)`, built first when
    /// it does not hold at the engine's clock. `read` says whether it read a
    /// hover that read the clock.
    fn reading<T>(
        &self,
        engine: &mut Engine<'_>,
        only: Option<&Path>,
        collection: Collection,
        diagnostics: DiagnosticSource,
        read: impl FnOnce(&mut Entry, &mut Engine<'_>) -> Result<(T, bool), String>,
    ) -> Result<T, String> {
        // Which diagnostics a collection holds is the caller's choice, so
        // that one collection is never shared.
        if collection == Collection::Diagnostics {
            let records = (self.collect)(self, collection, engine, only, diagnostics)?;
            let mut entry = Entry::new(engine.now(), false, records);
            return read(&mut entry, engine).map(|(value, _)| value);
        }
        let key = (only.map(Path::to_path_buf), collection.clone());
        let slot = self
            .state(engine.workspace())
            .entries
            .entry(key)
            .or_default()
            .clone();
        let mut entry = lock(&slot);
        let now = engine.now();
        if !entry.as_ref().is_some_and(|e| holds(e.now, e.exact, now)) {
            let mut own = engine.request().engine();
            let records = (self.collect)(self, collection, &mut own, only, diagnostics)?;
            let built_time_dependent = own.time_dependent();
            *entry = Some(Entry::new(now, built_time_dependent, records));
        }
        let entry = entry.as_mut().expect("filled above");
        // Read with an engine of its own, so what reading marks is this
        // read's alone and can decide how long the entry holds.
        let mut own = engine.request().engine();
        own.mark_time_dependent(entry.built_time_dependent);
        let (value, hover) = read(entry, &mut own)?;
        if (own.time_dependent() || hover) && !entry.exact {
            entry.exact = true;
            entry.now = now;
        }
        engine.mark_time_dependent(own.time_dependent());
        Ok(value)
    }

    /// What `module`'s `records` hook built for the note at `path`, built
    /// by `build` first when no build holds at the engine's clock. `engine`
    /// is marked when reading the module's inputs read the clock.
    pub(crate) fn built(
        &self,
        engine: &mut Engine<'_>,
        module: &Module,
        path: &Path,
        build: BuildRecords,
    ) -> Arc<Build> {
        let slot = self
            .state(engine.workspace())
            .builds
            .entry((path.to_path_buf(), module.id.clone()))
            .or_default()
            .clone();
        let mut entry = lock(&slot);
        let now = engine.now();
        if !entry.as_ref().is_some_and(|e| holds(e.now, e.exact, now)) {
            let mut own = engine.request().engine();
            let build = build(self, &mut own, module, path);
            *entry = Some(BuildEntry {
                now,
                exact: own.time_dependent(),
                build: Arc::new(build),
            });
        }
        let entry = entry.as_ref().expect("filled above");
        engine.mark_time_dependent(entry.exact);
        entry.build.clone()
    }

    /// The note at `path` as a hook is handed it.
    pub(crate) fn note(
        &self,
        workspace: &Workspace,
        path: &Path,
    ) -> Result<Arc<NoteValues>, String> {
        if let Some(note) = self.state(workspace).notes.get(path) {
            return Ok(note.clone());
        }
        let text = workspace.documents()[path].text();
        let note = Arc::new(NoteValues {
            uri: lang::common::file_url(path)?.into(),
            text: Value::Text(text.into()),
            lines: Value::list(text.lines().map(|l| Value::Text(l.into())).collect()),
        });
        self.state(workspace)
            .notes
            .insert(path.to_path_buf(), note.clone());
        Ok(note)
    }
    /// The state, emptied first when it was filled from another workspace.
    fn state(&self, workspace: &Workspace) -> MutexGuard<'_, State> {
        let mut state = lock(&self.state);
        let address = std::ptr::from_ref(workspace).addr();
        if state.workspace != address {
            state.workspace = address;
            state.entries.clear();
            state.builds.clear();
            state.notes.clear();
        }
        state
    }
}

/// Whether `view` reads the hover of any record, which may read the clock
/// without marking the reader.
fn reads_hover(records: &[Record], view: View<'_>) -> bool {
    let hover = crate::record::LazyField::Hover.as_str();
    let named = || {
        records
            .iter()
            .any(|r| r.fields.contains_key(crate::record::NAME))
    };
    match view {
        View::Full => false,
        View::Queried => named(),
        View::Fields(keys) => keys.iter().any(|k| k == hover) && named(),
    }
}
