//! The memo: evaluated results by what they are the value of, shared by every
//! engine of one request, and — when an owner such as an editor session keeps
//! an [`Evaluations`] — by the later requests over the same workspace
//! revision and clock.
//!
//! A request that reuses an earlier request's result must answer exactly as a
//! request of its own would have. Values, failures, lookups and clock
//! readings are the same wherever a definition is evaluated from, with three
//! exceptions, and those decide what may be shared:
//!
//! - **The clock.** A result that read the clock more finely than the date
//!   (`now()`, a `clocked` value still ticking, a resource whose cache ages)
//!   holds only at that instant; any other holds for the date and offset.
//!   Each kind lives in a layer of its own, and a new instant starts a new
//!   instant layer.
//! - **Where it was evaluated from.** A dependency cycle names the chain it
//!   was entered from, a depth limit counts the definitions and calls already
//!   in progress, and a failure placed at the caller's text belongs to that
//!   caller. Such a result is *contextual*: it stays with the request that
//!   made it, as does anything that reads it.
//! - **The step budget.** A request of its own evaluates each definition the
//!   first time it needs it, charging the steps to whatever needed it. So a
//!   result is recorded with how its evaluation spent steps ([`Event`]s), and
//!   a later request replays that charge — from the steps already spent,
//!   skipping what it has already evaluated itself — before it takes the
//!   result. When the replay would cross a limit (steps, depth or calls), or
//!   reach a definition already in progress, the request evaluates the
//!   definition for real, and meets the limit where a request of its own
//!   would.
use crate::{engine::EvalFailure, workspace::Symbol};
use chrono::{DateTime, FixedOffset};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
};
use values::{EvalResult, Value};

/// The expression nodes one evaluation may visit.
pub(crate) const STEP_LIMIT: usize = 200_000;
/// How many definitions may be in progress at once.
pub(crate) const DEPTH_LIMIT: usize = 64;
/// How many calls may be in progress at once.
pub(crate) const CALL_LIMIT: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum MemoKey {
    /// Shared, so making a key for a name read is a reference count.
    Symbol(Arc<Symbol>),
    ResourceProperty(String, String),
}

#[derive(Clone)]
pub(crate) struct MemoEntry {
    pub(crate) value: EvalResult<Value>,
    pub(crate) failure: Option<EvalFailure>,
    pub(crate) wanted: Vec<crate::lookups::LookupRead>,
    pub(crate) time_dependent: bool,
    /// Whether the result depends on where it was evaluated from, so only
    /// the request that made it may read it.
    pub(crate) contextual: bool,
    pub(crate) cost: Cost,
    /// What the module that evaluates a form definition said about it
    /// besides its value.
    pub(crate) about: Option<crate::forms::About>,
}

/// How evaluating a definition spent the budget, for a later request to
/// charge itself as evaluating it would have.
#[derive(Clone, Debug, Default)]
pub(crate) struct Cost {
    pub(crate) events: Vec<Event>,
    /// The most calls in progress at any depth check its own evaluation
    /// made, counted from where it started; `None` when it made none.
    pub(crate) call_peak: Option<usize>,
}

/// One thing a definition's evaluation did to the budget, in order.
#[derive(Clone, Debug)]
pub(crate) enum Event {
    /// Expression nodes visited.
    Steps(usize),
    /// A module engine started with nothing in progress, `calls` calls in
    /// from where the definition started: with no call in progress at all,
    /// the step count starts again there.
    Quiet { calls: usize },
    /// Another definition read, `calls` calls in.
    Reached { key: MemoKey, calls: usize },
}

/// The evaluation of one definition in progress: what it has spent and read
/// so far. An engine running module code on its behalf carries it too.
pub(crate) struct Walk {
    events: Vec<Event>,
    /// The step count the current run of [`Event::Steps`] started at.
    mark: usize,
    call_base: usize,
    call_peak: Option<usize>,
    /// How many contexts the engine had when the walk began: a failure placed
    /// at one of those belongs to the caller. `None` on a module engine,
    /// whose contexts are all its own.
    pub(crate) contexts_base: Option<usize>,
    /// The bases of the engines that lent the walk to a module engine.
    lent: Vec<Option<usize>>,
    pub(crate) contextual: bool,
}
impl Walk {
    pub(crate) fn new(steps: usize, calls: usize, contexts: usize) -> Self {
        Self {
            events: Vec::new(),
            mark: steps,
            call_base: calls,
            call_peak: None,
            contexts_base: Some(contexts),
            lent: Vec::new(),
            contextual: false,
        }
    }
    /// Carried onto a module engine, whose contexts are all its own.
    pub(crate) fn lend(mut self) -> Self {
        self.lent.push(self.contexts_base.take());
        self
    }
    /// Back from the module engine it was lent to.
    pub(crate) fn take_back(mut self) -> Self {
        self.contexts_base = self.lent.pop().flatten();
        self
    }
    /// End the current run of steps at `steps`.
    fn close(&mut self, steps: usize) {
        let spent = steps.saturating_sub(self.mark);
        if spent > 0 {
            self.events.push(Event::Steps(spent));
        }
        self.mark = steps;
    }
    /// Another definition is about to be read with `calls` calls in progress.
    pub(crate) fn reach(&mut self, key: MemoKey, steps: usize, calls: usize) {
        self.close(steps);
        let calls = calls.saturating_sub(self.call_base);
        self.events.push(Event::Reached { key, calls });
    }
    /// Evaluation resumes at `steps`, after a read or a restart.
    pub(crate) fn resume(&mut self, steps: usize) {
        self.mark = steps;
    }
    /// A module engine started with nothing in progress, at `steps`.
    pub(crate) fn quiet(&mut self, steps: usize, calls: usize) {
        self.close(steps);
        let calls = calls.saturating_sub(self.call_base);
        self.events.push(Event::Quiet { calls });
    }
    /// A depth check with `calls` calls in progress.
    pub(crate) fn call_check(&mut self, calls: usize) {
        let calls = calls.saturating_sub(self.call_base);
        self.call_peak = Some(self.call_peak.map_or(calls, |peak| peak.max(calls)));
    }
    pub(crate) fn finish(mut self, steps: usize) -> (Cost, bool) {
        self.close(steps);
        let cost = Cost {
            events: self.events,
            call_peak: self.call_peak,
        };
        (cost, self.contextual)
    }
}

type Entries = HashMap<MemoKey, Arc<MemoEntry>, std::hash::BuildHasherDefault<KeyHasher>>;

/// A memo key's hash, mixed a word at a time (the multiply-rotate scheme
/// rustc uses): keys are this process's own paths and indices, never input
/// an attacker chooses, and every name a note or module reads is looked up.
#[derive(Default)]
pub(crate) struct KeyHasher(u64);
impl KeyHasher {
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}
impl std::hash::Hasher for KeyHasher {
    fn write(&mut self, bytes: &[u8]) {
        let (words, rest) = bytes.as_chunks::<8>();
        for word in words {
            self.add(u64::from_le_bytes(*word));
        }
        let tail = rest
            .iter()
            .rev()
            .fold(0u64, |word, byte| word << 8 | u64::from(*byte));
        self.add(tail ^ rest.len() as u64);
    }
    fn write_u8(&mut self, n: u8) {
        self.add(n.into());
    }
    fn write_u32(&mut self, n: u32) {
        self.add(n.into());
    }
    fn write_u64(&mut self, n: u64) {
        self.add(n);
    }
    fn write_usize(&mut self, n: usize) {
        self.add(n as u64);
    }
    fn finish(&self) -> u64 {
        self.0
    }
}
type Layer = Arc<Mutex<Entries>>;

/// What earlier requests left: results that hold for the date and offset,
/// and results that hold only at the instant.
#[derive(Clone, Default)]
struct Layers {
    dated: Layer,
    instant: Layer,
}

/// The memo every engine of one request shares.
#[derive(Clone, Default)]
pub(crate) struct Memo {
    /// Everything this request evaluated or replayed: what a request of its
    /// own would hold by now, free to read.
    own: Arc<Mutex<Entries>>,
    /// What earlier requests left, when an owner keeps it.
    earlier: Option<Layers>,
}

/// A memo hit.
pub(crate) enum Found {
    /// Something this request already holds, free to read.
    Reached(Arc<MemoEntry>),
    /// An earlier request's result, which this request must charge itself
    /// for ([`Memo::replay`]) before reading it.
    Earlier(Arc<MemoEntry>),
}

/// Where a replay starts: the budget spent, the definitions in progress.
pub(crate) struct Start<'a> {
    pub(crate) steps: usize,
    pub(crate) step_limit: usize,
    pub(crate) calls: usize,
    pub(crate) stack: &'a [Symbol],
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Layers {
    fn find(&self, key: &MemoKey) -> Option<Arc<MemoEntry>> {
        let found = lock(&self.instant).get(key).cloned();
        found.or_else(|| lock(&self.dated).get(key).cloned())
    }
}

impl Memo {
    pub(crate) fn find(&self, key: &MemoKey) -> Option<Found> {
        if let Some(entry) = lock(&self.own).get(key) {
            return Some(Found::Reached(entry.clone()));
        }
        self.earlier.as_ref()?.find(key).map(Found::Earlier)
    }

    /// What the module of form definition `symbol` said about it besides its
    /// value, once it is evaluated.
    pub(crate) fn about(&self, symbol: &Symbol) -> Option<crate::forms::About> {
        let key = MemoKey::Symbol(Arc::new(symbol.clone()));
        match self.find(&key)? {
            Found::Reached(entry) | Found::Earlier(entry) => entry.about.clone(),
        }
    }

    /// Keep a result this request evaluated, for later requests too when it
    /// does not depend on where it was evaluated from.
    pub(crate) fn keep(&self, key: MemoKey, entry: MemoEntry) {
        let entry = Arc::new(entry);
        if let Some(layers) = &self.earlier
            && !entry.contextual
        {
            let layer = if entry.time_dependent {
                &layers.instant
            } else {
                &layers.dated
            };
            lock(layer).insert(key.clone(), entry.clone());
        }
        lock(&self.own).insert(key, entry);
    }

    /// Charge this request for evaluating `key` from `start` as a request of
    /// its own would: every definition it reaches that this request has not
    /// evaluated yet, in the order it reached them. Gives the step count
    /// that leaves, having taken them all as evaluated, or `None` — taking
    /// nothing — when evaluating them would cross a limit or reach a
    /// definition in progress, so the caller evaluates `key` for real.
    pub(crate) fn replay(&self, key: &MemoKey, start: Start<'_>) -> Option<usize> {
        let layers = self.earlier.as_ref()?;
        let mut own = lock(&self.own);
        let mut replay = Replay {
            layers,
            own: &own,
            stack: start.stack,
            visited: Entries::default(),
            steps: start.steps,
            step_limit: start.step_limit,
        };
        if !replay.visit(key, start.stack.len(), start.calls) {
            return None;
        }
        let (visited, steps) = (replay.visited, replay.steps);
        own.extend(visited);
        Some(steps)
    }
}

struct Replay<'a> {
    layers: &'a Layers,
    own: &'a Entries,
    stack: &'a [Symbol],
    visited: Entries,
    steps: usize,
    step_limit: usize,
}
impl Replay<'_> {
    /// Evaluate `key` at `depth` definitions and `calls` calls in progress,
    /// as the budget sees it. False when that would fail.
    fn visit(&mut self, key: &MemoKey, depth: usize, calls: usize) -> bool {
        if self.own.contains_key(key) || self.visited.contains_key(key) {
            return true;
        }
        let Some(entry) = self.layers.find(key) else {
            return false;
        };
        if depth >= DEPTH_LIMIT
            || matches!(key, MemoKey::Symbol(symbol) if self.stack.contains(&**symbol))
            || entry
                .cost
                .call_peak
                .is_some_and(|peak| calls + peak >= CALL_LIMIT)
        {
            return false;
        }
        self.visited.insert(key.clone(), entry.clone());
        for event in &entry.cost.events {
            match event {
                Event::Steps(n) => {
                    self.steps += n;
                    if self.steps > self.step_limit {
                        return false;
                    }
                }
                Event::Quiet { calls: at } => {
                    if calls + at == 0 {
                        self.steps = 0;
                    }
                }
                Event::Reached { key, calls: at } => {
                    if !self.visit(key, depth + 1, calls + at) {
                        return false;
                    }
                }
            }
        }
        true
    }
}

/// Evaluated results an owner keeps across requests: one workspace revision's,
/// for as long as the owner keeps that revision unchanged. Results that hold
/// for the date are kept all day; results that read the clock more finely,
/// only while the clock stands still. A new date or offset, or a different
/// workspace, starts over.
#[derive(Default)]
pub struct Evaluations {
    state: Mutex<Option<Generation>>,
}
struct Generation {
    /// The workspace the layers were filled from, by address: a guard, since
    /// the owner is what keeps it unchanged.
    workspace: usize,
    now: DateTime<FixedOffset>,
    layers: Layers,
}
impl Evaluations {
    /// The memo for a request over `workspace` at `now`.
    pub(crate) fn memo(&self, workspace: &crate::Workspace, now: DateTime<FixedOffset>) -> Memo {
        let address = std::ptr::from_ref(workspace).addr();
        let mut state = lock(&self.state);
        let layers = match &mut *state {
            Some(generation)
                if generation.workspace == address
                    && generation.now.offset() == now.offset()
                    && generation.now.date_naive() == now.date_naive() =>
            {
                if generation.now != now {
                    generation.now = now;
                    generation.layers.instant = Layer::default();
                }
                generation.layers.clone()
            }
            _ => state
                .insert(Generation {
                    workspace: address,
                    now,
                    layers: Layers::default(),
                })
                .layers
                .clone(),
        };
        Memo {
            own: Arc::default(),
            earlier: Some(layers),
        }
    }
}

thread_local! {
    static CLOCK_READ: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Run `f`, and say whether module code it called read the clock more finely
/// than the date. Module calls outside an evaluation (a feature module's
/// hooks, the stdlib presenting something) answer only with a value, so their
/// clock reading is reported here instead.
pub fn reads_clock<T>(f: impl FnOnce() -> T) -> (T, bool) {
    let outer = CLOCK_READ.with(|read| read.replace(false));
    let value = f();
    let read = CLOCK_READ.with(|read| read.replace(outer || read.get()));
    (value, read)
}

/// Module code called outside an evaluation read the clock.
pub(crate) fn clock_read() {
    CLOCK_READ.with(|read| read.set(true));
}

#[cfg(test)]
mod tests {
    use super::Evaluations;
    use crate::{RequestContext, Workspace, engine::Engine};
    use chrono::{DateTime, FixedOffset};
    use std::path::Path;

    const NOTE: &str = "/workspace/note.x.md";

    fn workspace(text: &str) -> Workspace {
        let mut workspace = Workspace::new(vec!["/workspace".into()]);
        workspace.insert_document(NOTE.into(), model::Document::parse(text.into()));
        workspace
    }
    fn at(now: &str) -> DateTime<FixedOffset> {
        DateTime::parse_from_rfc3339(now).unwrap()
    }
    /// What evaluating `name` answers, and the failure it reports.
    fn outcome(engine: &mut Engine<'_>, name: &str) -> String {
        let value = engine.named(Path::new(NOTE), name);
        format!("{value:?} {:?}", engine.failure())
    }

    /// What `name` evaluates to in a request of its own, and in each of
    /// `requests` sharing one owner's evaluations after it evaluated
    /// `first` — which must be the same, every time.
    fn same_as_fresh(text: &str, first: &str, name: &str) -> String {
        let ws = workspace(text);
        let now = at("2026-09-16T09:00:00-04:00");
        let fresh = outcome(&mut RequestContext::new(&ws, now).engine(), name);
        let evaluations = Evaluations::default();
        outcome(
            &mut RequestContext::sharing(&ws, now, &evaluations).engine(),
            first,
        );
        for _ in 0..2 {
            let shared = RequestContext::sharing(&ws, now, &evaluations);
            assert_eq!(outcome(&mut shared.engine(), name), fresh);
        }
        fresh
    }

    /// A request reuses what an earlier one evaluated — the very value, not
    /// a copy — for as long as the clock reads the same to it: all day, or,
    /// for what reads `now()`, only at that instant.
    #[test]
    fn results_are_reused_while_the_clock_they_read_holds() {
        let ws = workspace("steady := [1, 2]\nstamp := [now()]\n");
        let evaluations = Evaluations::default();
        let list = |now: &str, name: &str| {
            let request = RequestContext::sharing(&ws, at(now), &evaluations);
            match request.engine().named(Path::new(NOTE), name) {
                Ok(values::Value::List(items)) => items,
                other => panic!("expected a list, found {other:?}"),
            }
        };
        let morning = "2026-09-16T09:00:00-04:00";
        let noon = "2026-09-16T12:00:00-04:00";
        let same = std::sync::Arc::ptr_eq;
        assert!(same(&list(morning, "steady"), &list(morning, "steady")));
        assert!(same(&list(morning, "steady"), &list(noon, "steady")));
        assert!(same(&list(morning, "stamp"), &list(morning, "stamp")));
        assert!(!same(&list(morning, "stamp"), &list(noon, "stamp")));
        assert_ne!(list(morning, "stamp"), list(noon, "stamp"));
        let tomorrow = "2026-09-17T09:00:00-04:00";
        let elsewhere = "2026-09-16T13:00:00Z";
        assert!(!same(&list(morning, "steady"), &list(tomorrow, "steady")));
        assert!(!same(&list(morning, "steady"), &list(elsewhere, "steady")));
    }

    /// Reusing a result charges the step budget as evaluating it would have:
    /// `b` alone crosses the limit, although `b` with `a` already evaluated
    /// in the same request does not.
    #[test]
    fn a_reused_result_costs_the_steps_evaluating_it_would() {
        let fold = "fold(xs, 0, fn(n, x) => n + 1)";
        let text = format!(
            "xs := split(repeat(\"x,\", 8000), \",\")\n\
             a := {fold} + {fold} + {fold}\n\
             b := a + {fold} + {fold}\n"
        );
        let answer = same_as_fresh(&text, "a", "b");
        assert!(answer.contains("StepLimit"), "{answer}");
        let ws = workspace(&text);
        let mut engine = RequestContext::new(&ws, at("2026-09-16T09:00:00-04:00")).engine();
        outcome(&mut engine, "a");
        assert!(outcome(&mut engine, "b").starts_with("Ok"));
    }

    /// A cycle is named from where it was entered, and a chain too deep for
    /// the definitions already in progress fails there: neither is handed to
    /// a request that enters it elsewhere.
    #[test]
    fn where_a_result_was_evaluated_from_stays_with_it() {
        let cycle = same_as_fresh("a := b\nb := a\n", "b", "a");
        assert!(cycle.contains("\"a\", \"b\", \"a\""), "{cycle}");
        let chain: String = (0..70)
            .map(|i| format!("d{i} := d{} + 1\n", i + 1))
            .chain(["d70 := 1\n".into()])
            .collect();
        let deep = same_as_fresh(&chain, "d30", "d0");
        assert!(deep.contains("DepthExceeded"), "{deep}");
        assert!(same_as_fresh(&chain, "d30", "d20").starts_with("Ok"));
    }
}
