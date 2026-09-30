//! The contract between native code and the standard library: every function
//! the engine, the language services or an editor calls in a bundled .xmd
//! module, declared once in [`CONTRACT`] and called only through the typed
//! functions below, one namespace per module.
//!
//! Native code never names a stdlib function by string anywhere else. A
//! module that replaces a bundled id is checked against this table when the
//! registry links, and `book/reference/contract.md` is generated from it.
//!
//! A call runs through a [`Caller`]: inside an evaluation (the [`Engine`],
//! sharing its memo, clock and budget), against a registry snapshot
//! ([`Snapshot`]), or on one module a value holds directly ([`Held`]).
use crate::engine::Engine;
use chrono::{DateTime, FixedOffset};
use modules::{Module, ModuleRegistry};
use std::path::PathBuf;
use values::{EvalResult, Value};

/// What native code relies on a call for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Words or markup shown to a person; a failure costs only a label. Where
    /// the words go, the person sees `fallback`, a neutral stand-in the typed
    /// function builds ([`Presented`]), and the note's diagnostics say why;
    /// the error text never stands in for the label.
    Presents { fallback: &'static str },
    /// Behavior the engine or an editor acts on: a record, a decision or edits.
    /// A failure reaches the person as a diagnostic, an error result or a
    /// disabled control carrying the reason, never as a quiet default.
    Decides,
}

/// One function native code calls in a stdlib module.
#[derive(Clone, Copy, Debug)]
pub struct Contract {
    pub module: &'static str,
    pub function: &'static str,
    /// Each parameter as `name: kind`.
    pub params: &'static [&'static str],
    pub returns: &'static str,
    pub role: Role,
    /// Whether the module must define it. An optional function has a native
    /// fallback when it is absent.
    pub required: bool,
    /// Whether the call runs at the request's clock. One without a clock is
    /// handed every date it needs as an argument, and `now()` or `today()`
    /// inside it fails rather than answering 1970 ([`modules::no_clock`]).
    pub clock: bool,
    pub doc: &'static str,
}

const fn entry(
    module: &'static str,
    function: &'static str,
    params: &'static [&'static str],
    returns: &'static str,
    role: Role,
    doc: &'static str,
) -> Contract {
    Contract {
        module,
        function,
        params,
        returns,
        role,
        required: true,
        clock: true,
        doc,
    }
}

const fn optional(contract: Contract) -> Contract {
    Contract {
        required: false,
        ..contract
    }
}

/// An entry native code calls without a clock: see [`Contract::clock`].
const fn clockless(contract: Contract) -> Contract {
    Contract {
        clock: false,
        ..contract
    }
}

/// A `Presents` role that shows `fallback` when the call fails.
const fn presents(fallback: &'static str) -> Role {
    Role::Presents { fallback }
}

use Role::Decides;

/// Every stdlib function native code calls, by module.
pub static CONTRACT: &[Contract] = &[
    entry(
        "format",
        "series",
        &["values: List"],
        "Text or Null",
        presents("no chart"),
        "A sparkline for a list of values, or null when nothing in it can be charted.",
    ),
    entry(
        "format",
        "age",
        &["elapsed: Duration"],
        "Text",
        presents("the elapsed duration"),
        "How long ago cached data was fetched, in the coarsest fitting unit.",
    ),
    entry(
        "format",
        "glyph",
        &["name: Text"],
        "Text",
        presents("the glyph's name"),
        "The glyph a control title starts with.",
    ),
    entry(
        "today",
        "page",
        &["entries: List", "day: Date"],
        "Markdown",
        presents("nothing; the today command fails and says why"),
        "The today page: the agenda entries laid out for one day.",
    ),
    entry(
        "task",
        "checklist",
        &["done: Count", "total: Count"],
        "Text",
        presents("`done/total`"),
        "The progress words a named heading's hover shows for its tasks.",
    ),
    entry(
        "task",
        "hover",
        &["task: tasks record"],
        "Markdown",
        presents("the task's title"),
        "A task's hover: its state, blockers, estimate, timer and subtasks, from its `tasks` record as queries and feature modules see it.",
    ),
    entry(
        "task",
        "toggle",
        &["recurring: Boolean", "done: Boolean"],
        "Text",
        presents("`toggle`"),
        "The title of the control that completes, reopens or advances a task.",
    ),
    entry(
        "resource",
        "label",
        &["resource: resource record"],
        "Text",
        presents("the resource's target"),
        "A resource's inline label when no link module recognizes it.",
    ),
    entry(
        "resource",
        "hover",
        &["resource: resource record"],
        "Markdown",
        presents("the resource's target"),
        "A resource's hover; a link module that recognizes the resource adds its details after it.",
    ),
    entry(
        "resource",
        "control",
        &["resource: resource record"],
        "Text",
        presents("the resource's target"),
        "The title of the control that opens a resource.",
    ),
    clockless(entry(
        "itinerary_core",
        "dates",
        &["days: List", "today: Date"],
        "List of Date or Null",
        Decides,
        "The calendar date of each itinerary day, inferring years and steps.",
    )),
    clockless(entry(
        "itinerary_core",
        "time_text",
        &["stop: stop record"],
        "Text",
        presents("the time as `HH:MM`"),
        "A stop's time as written in its label.",
    )),
    clockless(entry(
        "itinerary_core",
        "label",
        &["stop: stop record"],
        "Text",
        presents("the stop's title"),
        "A stop's inline label.",
    )),
    entry(
        "timer",
        "create",
        &["name: Text", "args: List"],
        "timer record",
        Decides,
        "A new timer from `countdown(...)` or `stopwatch(...)` and its arguments.",
    ),
    optional(entry(
        "timer",
        "time_dependent",
        &["timer: timer record"],
        "Boolean",
        Decides,
        "Whether the timer's display changes with the clock; without it, the module's `live` flag.",
    )),
    entry(
        "timer",
        "state",
        &["timer: timer record"],
        "Text",
        Decides,
        "The timer's state: idle, running, paused or done.",
    ),
    entry(
        "timer",
        "display",
        &["timer: timer record"],
        "Text",
        presents("nothing"),
        "The timer's inline display.",
    ),
    entry(
        "timer",
        "hover",
        &["timer: timer record"],
        "Markdown",
        presents("nothing"),
        "The timer's hover.",
    ),
    entry(
        "timer",
        "property",
        &["timer: timer record", "name: Text"],
        "Value",
        Decides,
        "A property a note reads from the timer, such as `.remaining`.",
    ),
    entry(
        "timer",
        "transition",
        &["timer: timer record", "action: Text", "original: Text"],
        "Text",
        Decides,
        "The timer's new source expression after a start, pause, resume or reset.",
    ),
    entry(
        "plan",
        "solve_model",
        &["model: plan model record"],
        "solution record",
        Decides,
        "A plan's solution: decisions, constraint slack and status.",
    ),
    entry(
        "plan",
        "seek_boundary",
        &["name: Text", "form: linear form record", "unit: Value"],
        "Value",
        Decides,
        "The value of a goal-seek unknown at the boundary its constraint sets.",
    ),
    entry(
        "plan",
        "hover",
        &["plan: plan record"],
        "Markdown",
        presents("nothing"),
        "A plan's hover.",
    ),
    entry(
        "plan",
        "seek_summary",
        &["op: Text", "positive: Boolean"],
        "Text",
        presents("the constraint's operator"),
        "The words a goal seek's hover uses for its direction.",
    ),
    entry(
        "plan",
        "write_edits",
        &["plan: plan record", "document: Text"],
        "List of text edits",
        Decides,
        "The edits that write a plan's solved decisions into its note.",
    ),
    entry(
        "plan",
        "write_title",
        &[],
        "Text",
        presents("`write decisions`"),
        "The title of the code action that writes a plan's decisions.",
    ),
];

/// Every module id the contract names, once each, whatever order its
/// entries are in.
pub fn modules() -> Vec<&'static str> {
    let ids: std::collections::BTreeSet<_> = CONTRACT.iter().map(|c| c.module).collect();
    ids.into_iter().collect()
}

/// The contract entry for `module.function`, if there is one.
pub(crate) fn contract(module: &str, function: &str) -> Option<&'static Contract> {
    CONTRACT
        .iter()
        .find(|c| c.module == module && c.function == function)
}

/// Where a contract call runs.
pub trait Caller {
    fn call_stdlib(&mut self, module: &str, function: &str, args: Vec<Value>) -> EvalResult<Value>;
    /// Whether the call runs at a real clock rather than
    /// [`modules::no_clock`].
    fn has_clock(&self) -> bool;
}

impl Caller for Engine<'_> {
    fn call_stdlib(&mut self, module: &str, function: &str, args: Vec<Value>) -> EvalResult<Value> {
        self.call_module(module, function, args)
    }
    fn has_clock(&self) -> bool {
        Engine::has_clock(self)
    }
}

/// The registry at one moment, for callers outside an evaluation.
pub struct Snapshot<'a> {
    pub modules: &'a ModuleRegistry,
    pub now: DateTime<FixedOffset>,
}
impl<'a> Snapshot<'a> {
    /// The registry with no clock, for the entries [`CONTRACT`] marks as
    /// called without one.
    pub(crate) fn clockless(modules: &'a ModuleRegistry) -> Self {
        Self {
            modules,
            now: modules::no_clock(),
        }
    }
}
impl Caller for Snapshot<'_> {
    fn call_stdlib(&mut self, module: &str, function: &str, args: Vec<Value>) -> EvalResult<Value> {
        self.modules.call(module, function, args, self.now)
    }
    fn has_clock(&self) -> bool {
        modules::has_clock(self.now)
    }
}

/// One module a value holds on to, such as the implementation a timer was
/// created with.
pub(crate) struct Held<'a> {
    pub(crate) module: &'a Module,
    pub(crate) now: DateTime<FixedOffset>,
}
impl Caller for Held<'_> {
    fn call_stdlib(&mut self, module: &str, function: &str, args: Vec<Value>) -> EvalResult<Value> {
        debug_assert_eq!(
            module, self.module.id,
            "a held module answers only for itself"
        );
        self.module.call(function, args, self.now)
    }
    fn has_clock(&self) -> bool {
        modules::has_clock(self.now)
    }
}

/// Call `module.function`, which must be in [`CONTRACT`] with this many
/// arguments: every typed function below goes through here.
fn call(
    caller: &mut impl Caller,
    module: &str,
    function: &str,
    args: Vec<Value>,
) -> EvalResult<Value> {
    let entry = contract(module, function);
    debug_assert!(
        entry.is_some(),
        "{module}.{function} is not in the stdlib contract"
    );
    debug_assert_eq!(
        entry.map(|c| c.params.len()),
        Some(args.len()),
        "{module}.{function} takes the arguments its contract lists"
    );
    debug_assert!(
        entry.is_none_or(|c| !c.clock || caller.has_clock()),
        "{module}.{function} runs at the request's clock, so its caller needs one"
    );
    caller.call_stdlib(module, function, args)
}

/// A `Presents` call's answer, and the neutral stand-in shown where the
/// answer goes when the call fails. The contract entry's role describes the
/// fallback; the typed function builds it from the call's own inputs.
#[must_use]
pub struct Presented<T = String> {
    /// The contract entry that was called.
    pub entry: &'static Contract,
    pub result: EvalResult<T>,
    pub fallback: T,
}

/// A `Presents` call's text, or its fallback: the one place a failed
/// presentation becomes what a person sees. The error is the note's
/// diagnostics' to report (`catalog::presentations`), never the label's.
pub fn shown<T>(presented: Presented<T>) -> T {
    presented.result.unwrap_or(presented.fallback)
}

/// Call a `Presents` entry, answering `fallback` when it fails.
fn present<T>(
    caller: &mut impl Caller,
    (module, function): (&str, &str),
    args: Vec<Value>,
    answer: impl FnOnce(Value) -> EvalResult<T>,
    fallback: T,
) -> Presented<T> {
    let entry = contract(module, function)
        .unwrap_or_else(|| panic!("{module}.{function} is not in the stdlib contract"));
    debug_assert!(
        matches!(entry.role, Role::Presents { .. }),
        "{module}.{function} decides, so its failure is not a label's"
    );
    Presented {
        entry,
        result: call(caller, module, function, args).and_then(answer),
        fallback,
    }
}

/// A presented `Text` answer, read as its display.
fn display(value: Value) -> EvalResult<String> {
    Ok(value.display())
}

/// A record's field as text, for a fallback built from a call's input.
fn field(record: &Value, name: &str) -> String {
    match record {
        Value::Record(fields) => fields.get(name).map(Value::display).unwrap_or_default(),
        _ => String::new(),
    }
}

/// Every way the active modules in `modules` break [`CONTRACT`], as the
/// module file and a message. A module whose id is in the contract must
/// define each required function, and every contract function it defines must
/// be a function taking the listed number of arguments. An id the registry
/// does not hold (or holds disabled) has nothing to check: calls to it fail
/// as unavailable.
pub(crate) fn check(modules: &ModuleRegistry) -> Vec<(PathBuf, String)> {
    let mut problems = Vec::new();
    for module in self::modules().into_iter().filter_map(|id| modules.get(id)) {
        let defined = module.member_names();
        let mut named = module.environment().evaluator(&module.path);
        for entry in CONTRACT.iter().filter(|c| c.module == module.id) {
            let problem = |detail: String| {
                format!(
                    "{}.{} is part of the stdlib contract ({}({}) -> {}) {detail}",
                    entry.module,
                    entry.function,
                    entry.function,
                    entry.params.join(", "),
                    entry.returns,
                )
            };
            let arguments = |n: usize| match n {
                1 => "1 argument".to_owned(),
                n => format!("{n} arguments"),
            };
            if !defined.iter().any(|name| name == entry.function) {
                if entry.required {
                    problems.push((
                        module.path.clone(),
                        problem("but this module does not define it".into()),
                    ));
                }
                continue;
            }
            let message = match named(entry.function) {
                Ok(Value::Function(f)) if f.params.len() == entry.params.len() => continue,
                Ok(Value::Function(f)) => problem(format!(
                    "and takes {}; this module's takes {}",
                    arguments(entry.params.len()),
                    f.params.len()
                )),
                Ok(other) => problem(format!(
                    "but this module defines it as {}, not a function",
                    other.type_name()
                )),
                Err(e) => problem(format!("but this module's definition fails: {e}")),
            };
            problems.push((module.path.clone(), message));
        }
    }
    problems
}

/// A `Text` answer, read as its display.
fn text(result: EvalResult<Value>) -> EvalResult<String> {
    result.map(|v| v.display())
}

pub mod format {
    use super::*;
    /// A sparkline for `values`, or `None` when nothing in it can be charted.
    pub fn series(caller: &mut impl Caller, values: Vec<Value>) -> Presented<Option<String>> {
        present(
            caller,
            ("format", "series"),
            vec![Value::list(values)],
            |chart| {
                Ok(match chart {
                    Value::Null => None,
                    chart => Some(chart.display()),
                })
            },
            None,
        )
    }
    /// How long ago, for data fetched `seconds` ago.
    pub fn age(caller: &mut impl Caller, seconds: i64) -> Presented {
        let elapsed = Value::Duration(seconds);
        let fallback = elapsed.display();
        present(caller, ("format", "age"), vec![elapsed], display, fallback)
    }
    /// The glyph a control title starts with.
    pub fn glyph(caller: &mut impl Caller, name: &str) -> Presented {
        present(
            caller,
            ("format", "glyph"),
            vec![Value::Text(name.into())],
            display,
            name.into(),
        )
    }
}

pub mod today {
    use super::*;
    /// The today page for `day`, from its agenda entries. The page is the
    /// whole answer, so a failure is the today command's error.
    pub fn page(
        caller: &mut impl Caller,
        entries: Vec<Value>,
        day: chrono::NaiveDate,
    ) -> EvalResult<String> {
        text(call(
            caller,
            "today",
            "page",
            vec![Value::list(entries), Value::Date(day)],
        ))
    }
}

pub mod task {
    use super::*;
    /// The progress words for `done` of `total` tasks under a named heading.
    pub fn checklist(caller: &mut impl Caller, done: usize, total: usize) -> Presented {
        present(
            caller,
            ("task", "checklist"),
            vec![Value::Count(done), Value::Count(total)],
            display,
            format!("{done}/{total}"),
        )
    }
    /// A task's hover, from its task record.
    pub fn hover(caller: &mut impl Caller, task: Value) -> Presented {
        let fallback = field(&task, "title");
        present(caller, ("task", "hover"), vec![task], display, fallback)
    }
    /// The title of the control that completes, reopens or advances a task.
    pub fn toggle(caller: &mut impl Caller, recurring: bool, done: bool) -> Presented {
        present(
            caller,
            ("task", "toggle"),
            vec![Value::Bool(recurring), Value::Bool(done)],
            display,
            "toggle".into(),
        )
    }
}

pub mod resource {
    use super::*;
    /// A resource's inline label, from its resource record.
    pub fn label(caller: &mut impl Caller, resource: Value) -> Presented {
        let fallback = field(&resource, "target");
        present(
            caller,
            ("resource", "label"),
            vec![resource],
            display,
            fallback,
        )
    }
    /// A resource's hover, from its resource record.
    pub fn hover(caller: &mut impl Caller, resource: Value) -> Presented {
        let fallback = field(&resource, "target");
        present(
            caller,
            ("resource", "hover"),
            vec![resource],
            display,
            fallback,
        )
    }
    /// The title of the control that opens a resource.
    pub fn control(caller: &mut impl Caller, resource: Value) -> Presented {
        let fallback = field(&resource, "target");
        present(
            caller,
            ("resource", "control"),
            vec![resource],
            display,
            fallback,
        )
    }
}

pub(crate) mod itinerary_core {
    use super::*;
    /// The calendar date of each day, from its calendar parts; `None` for a
    /// day the module gives no date.
    pub(crate) fn dates(
        caller: &mut impl Caller,
        days: Vec<Value>,
        today: chrono::NaiveDate,
    ) -> EvalResult<Vec<Option<chrono::NaiveDate>>> {
        let result = call(
            caller,
            "itinerary_core",
            "dates",
            vec![Value::list(days), Value::Date(today)],
        )?;
        Ok(values::list(&result)?
            .iter()
            .map(|v| match v {
                Value::Date(d) => Some(*d),
                _ => None,
            })
            .collect())
    }
    /// A stop's time as written in its label, from its stop record.
    pub(crate) fn time_text(caller: &mut impl Caller, stop: Value) -> Presented {
        let fallback = match &stop {
            Value::Record(fields) => match fields.get("time") {
                Some(Value::Duration(seconds)) => {
                    let minutes = seconds / 60;
                    format!("{:02}:{:02}", minutes / 60, minutes % 60)
                }
                _ => String::new(),
            },
            _ => String::new(),
        };
        present(
            caller,
            ("itinerary_core", "time_text"),
            vec![stop],
            display,
            fallback,
        )
    }
    /// A stop's inline label, from its stop record.
    pub(crate) fn label(caller: &mut impl Caller, stop: Value) -> Presented {
        let fallback = field(&stop, "title");
        present(
            caller,
            ("itinerary_core", "label"),
            vec![stop],
            display,
            fallback,
        )
    }
}

pub(crate) mod timer {
    use super::*;
    /// A new timer record from `countdown(...)` or `stopwatch(...)` and its
    /// arguments.
    pub(crate) fn create(
        caller: &mut impl Caller,
        name: &str,
        args: Vec<Value>,
    ) -> EvalResult<Value> {
        call(
            caller,
            "timer",
            "create",
            vec![Value::Text(name.into()), Value::list(args)],
        )
    }
    /// Whether the timer's display changes with the clock. Optional: callers
    /// fall back to the module's `live` flag when it is absent.
    pub(crate) fn time_dependent(caller: &mut impl Caller, timer: Value) -> EvalResult<bool> {
        match call(caller, "timer", "time_dependent", vec![timer])? {
            Value::Bool(live) => Ok(live),
            _ => Err(values::EvalError::Message(
                "timer.time_dependent must return a boolean".into(),
            )),
        }
    }
    /// The timer's state as the module names it: idle, running, paused or done.
    pub(crate) fn state(caller: &mut impl Caller, timer: Value) -> EvalResult<String> {
        text(call(caller, "timer", "state", vec![timer]))
    }
    /// The timer's inline display.
    pub(crate) fn display(caller: &mut impl Caller, timer: Value) -> Presented {
        present(
            caller,
            ("timer", "display"),
            vec![timer],
            super::display,
            String::new(),
        )
    }
    /// The timer's hover.
    pub(crate) fn hover(caller: &mut impl Caller, timer: Value) -> Presented {
        present(
            caller,
            ("timer", "hover"),
            vec![timer],
            super::display,
            String::new(),
        )
    }
    /// A property a note reads from the timer, such as `.remaining`.
    pub(crate) fn property(
        caller: &mut impl Caller,
        timer: Value,
        name: &str,
    ) -> EvalResult<Value> {
        call(
            caller,
            "timer",
            "property",
            vec![timer, Value::Text(name.into())],
        )
    }
    /// The timer's new source expression after `action`, from the first
    /// argument it was `original`ly declared with.
    pub(crate) fn transition(
        caller: &mut impl Caller,
        timer: Value,
        action: &str,
        original: &str,
    ) -> EvalResult<String> {
        text(call(
            caller,
            "timer",
            "transition",
            vec![
                timer,
                Value::Text(action.into()),
                Value::Text(original.into()),
            ],
        ))
    }
}

pub mod plan {
    use super::*;
    /// A plan model's solution record: decisions, constraint slack and status.
    pub(crate) fn solve_model(caller: &mut impl Caller, model: Value) -> EvalResult<Value> {
        call(caller, "plan", "solve_model", vec![model])
    }
    /// The value of the goal-seek unknown `name` at the boundary its
    /// constraint's linear `form` sets, in `unit`.
    pub(crate) fn seek_boundary(
        caller: &mut impl Caller,
        name: &str,
        form: Value,
        unit: Value,
    ) -> EvalResult<Value> {
        call(
            caller,
            "plan",
            "seek_boundary",
            vec![Value::Text(name.into()), form, unit],
        )
    }
    /// A solved plan's hover.
    pub fn hover(caller: &mut impl Caller, plan: Value) -> Presented {
        present(
            caller,
            ("plan", "hover"),
            vec![plan],
            display,
            String::new(),
        )
    }
    /// The words a goal seek's hover uses for its direction.
    pub fn seek_summary(caller: &mut impl Caller, op: &str, positive: bool) -> Presented {
        present(
            caller,
            ("plan", "seek_summary"),
            vec![Value::Text(op.into()), Value::Bool(positive)],
            display,
            op.into(),
        )
    }
    /// The text edits, as the module writes them, that put a plan's solved
    /// decisions into the note at `document`.
    pub fn write_edits(caller: &mut impl Caller, plan: Value, document: &str) -> EvalResult<Value> {
        call(
            caller,
            "plan",
            "write_edits",
            vec![plan, Value::Text(document.into())],
        )
    }
    /// The title of the code action that writes a plan's decisions.
    pub fn write_title(caller: &mut impl Caller) -> Presented {
        present(
            caller,
            ("plan", "write_title"),
            vec![],
            display,
            "write decisions".into(),
        )
    }
}
