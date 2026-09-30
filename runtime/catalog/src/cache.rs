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
//!   reads the clock more finely, the entry holds only for that instant.
//!
//! Lazy fields stay lazy: a reader that narrows a collection to a few fields
//! evaluates only those, and what they produce is kept on the cached record
//! for the next reader.
use crate::{Collection, DiagnosticSource, Record};
use chrono::{DateTime, FixedOffset};
use lang::eval::Workspace;
use lang::eval::engine::{Engine, Value};
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

/// Derived records for one workspace, shared by every reader of it.
#[derive(Default)]
pub struct Records {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    /// The workspace the entries were built from, by address: a guard, since
    /// the owner is what keeps it unchanged.
    workspace: usize,
    entries: BTreeMap<Key, Arc<Mutex<Option<Entry>>>>,
}

struct Entry {
    /// The clock the entry answers for.
    now: DateTime<FixedOffset>,
    /// Whether building the records read the clock: replayed onto every
    /// reader's engine, as building them again would mark it.
    built_time_dependent: bool,
    /// Whether anything in the entry reads the clock more finely than the
    /// date, so the entry holds only at `now`. Hovers count, though they
    /// never mark a reader's engine: they word lookup ages and host state.
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
    fn holds_at(&self, now: DateTime<FixedOffset>) -> bool {
        let same_offset = self.now.offset() == now.offset();
        if self.exact {
            same_offset && self.now == now
        } else {
            same_offset && self.now.date_naive() == now.date_naive()
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Records {
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
            Ok((value, reads_hover(&entry.records, view)))
        })
    }

    /// The `index`th record of `collection` in the note at `path`, every
    /// field read: one task's record for its hover, say.
    pub(crate) fn nth(
        &self,
        engine: &mut Engine<'_>,
        path: &Path,
        collection: Collection,
        index: usize,
    ) -> Option<Value> {
        self.reading(
            engine,
            Some(path),
            collection,
            no_diagnostics,
            |entry, engine| {
                let record = entry.records.get_mut(index).map(|r| r.full(engine));
                Ok((record, false))
            },
        )
        .ok()
        .flatten()
    }

    /// Run `read` over the entry for `(only, collection)`, built first when
    /// it does not hold at the engine's clock. `read` says whether it read a
    /// hover.
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
            let ws = engine.workspace();
            let records = crate::collect(ws, collection, engine, only, diagnostics)?;
            let mut entry = Entry::new(engine.now(), false, records);
            return read(&mut entry, engine).map(|(value, _)| value);
        }
        let key = (only.map(Path::to_path_buf), collection);
        let slot = self.slot(engine.workspace(), key);
        let mut entry = lock(&slot);
        let now = engine.now();
        if !entry.as_ref().is_some_and(|e| e.holds_at(now)) {
            let mut own = engine.request().engine();
            let records =
                crate::collect(engine.workspace(), collection, &mut own, only, diagnostics)?;
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

    fn slot(&self, workspace: &Workspace, key: Key) -> Arc<Mutex<Option<Entry>>> {
        let mut state = lock(&self.state);
        let address = std::ptr::from_ref(workspace).addr();
        if state.workspace != address {
            state.workspace = address;
            state.entries.clear();
        }
        state.entries.entry(key).or_default().clone()
    }
}

/// For a read that never asks for the diagnostics collection.
fn no_diagnostics(_: &lang::eval::RequestContext<'_>, _: &Path) -> Vec<lsp_types::Diagnostic> {
    Vec::new()
}

/// Whether `view` reads the hover of any record, which words the clock
/// without marking it.
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
