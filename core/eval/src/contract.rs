//! The contract between native code and the standard library: every function
//! the engine, the language services or an editor calls in a bundled .xmd
//! module, declared once in [`CONTRACT`] and called only through the typed
//! functions below, one namespace per module.
//!
//! Native code never names a stdlib function by string anywhere else. A
//! module that replaces a bundled id is checked against this table when the
//! registry links, and `book/reference/contract.md` is generated from it.
//!
//! A call runs inside an evaluation: the [`Engine`] it is handed, sharing
//! its memo, clock and budget.
use crate::engine::Engine;
use modules::ModuleRegistry;
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
        doc,
    }
}

const fn optional(contract: Contract) -> Contract {
    Contract {
        required: false,
        ..contract
    }
}

/// A `Presents` role that shows `fallback` when the call fails.
const fn presents(fallback: &'static str) -> Role {
    Role::Presents { fallback }
}

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
    optional(entry(
        "prelude",
        "lookup_display",
        &[
            "kind: Text",
            "key: List of one-field records",
            "value: Value",
        ],
        "Text",
        presents("the cached value"),
        "How the cached value of a lookup a module's record asked for reads, or why it cannot be read.",
    )),
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

/// Call `module.function`, which must be in [`CONTRACT`] with this many
/// arguments: every typed function below goes through here.
fn call(
    caller: &mut Engine<'_>,
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
    caller.call_module(module, function, args)
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
/// diagnostics' to report (`records::presentations`), never the label's.
pub fn shown<T>(presented: Presented<T>) -> T {
    presented.result.unwrap_or(presented.fallback)
}

/// Call a `Presents` entry, answering `fallback` when it fails.
fn present<T>(
    caller: &mut Engine<'_>,
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

pub mod format {
    use super::*;
    /// A sparkline for `values`, or `None` when nothing in it can be charted.
    pub fn series(caller: &mut Engine<'_>, values: Vec<Value>) -> Presented<Option<String>> {
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
    pub fn age(caller: &mut Engine<'_>, seconds: i64) -> Presented {
        let elapsed = Value::Duration(seconds);
        let fallback = elapsed.display();
        present(caller, ("format", "age"), vec![elapsed], display, fallback)
    }
    /// The glyph a control title starts with.
    pub fn glyph(caller: &mut Engine<'_>, name: &str) -> Presented {
        present(
            caller,
            ("format", "glyph"),
            vec![Value::Text(name.into())],
            display,
            name.into(),
        )
    }
}

pub mod prelude {
    use super::*;
    use values::{Lookup, LookupKey};
    /// How the cached `lookup` of `key` reads where a module's record asked
    /// for it, or why it cannot be read.
    pub fn lookup_display(caller: &mut Engine<'_>, key: &LookupKey, lookup: &Lookup) -> Presented {
        let value = values::from_json(&lookup.value);
        let fallback = value.display();
        present(
            caller,
            ("prelude", "lookup_display"),
            vec![Value::Text(key.kind().into()), key.key(), value],
            display,
            fallback,
        )
    }
}

pub mod today {
    use super::*;
    /// The today page for `day`, from its agenda entries. The page is the
    /// whole answer, so a failure is the today command's error.
    pub fn page(
        caller: &mut Engine<'_>,
        entries: Vec<Value>,
        day: chrono::NaiveDate,
    ) -> EvalResult<String> {
        call(
            caller,
            "today",
            "page",
            vec![Value::list(entries), Value::Date(day)],
        )
        .and_then(display)
    }
}

pub mod task {
    use super::*;
    /// The progress words for `done` of `total` tasks under a named heading.
    pub fn checklist(caller: &mut Engine<'_>, done: usize, total: usize) -> Presented {
        present(
            caller,
            ("task", "checklist"),
            vec![Value::Count(done), Value::Count(total)],
            display,
            format!("{done}/{total}"),
        )
    }
}

pub mod resource {
    use super::*;
    /// A resource's inline label, from its resource record.
    pub fn label(caller: &mut Engine<'_>, resource: Value) -> Presented {
        presented(caller, "label", resource)
    }
    /// A resource's hover, from its resource record.
    pub fn hover(caller: &mut Engine<'_>, resource: Value) -> Presented {
        presented(caller, "hover", resource)
    }
    /// The title of the control that opens a resource.
    pub fn control(caller: &mut Engine<'_>, resource: Value) -> Presented {
        presented(caller, "control", resource)
    }
    /// `resource.function` of a resource record, which falls back to the
    /// resource's target.
    fn presented(caller: &mut Engine<'_>, function: &str, resource: Value) -> Presented {
        let fallback = field(&resource, "target");
        present(
            caller,
            ("resource", function),
            vec![resource],
            display,
            fallback,
        )
    }
}
