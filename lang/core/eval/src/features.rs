//! The registry of feature evaluators: the one list of what each feature
//! answers for the engine. The engine evaluates expressions, names, calls and
//! the core special forms (`if`, `match`, `let`, `eval`, `import`, …); a
//! definition the model knows as a feature ([`DefinitionKind`]) and a special
//! built-in the engine does not answer itself are looked up here, so the
//! engine never names a feature and every feature depends on the engine
//! rather than the other way round. A new feature is one row.
use crate::engine::{Builtin, Engine, Expr, Value};
use crate::workspace::Symbol;
use crate::{checklists, lookups, plans, tables_impl, timers};
use model::DefinitionKind;
use std::path::Path;
use values::EvalResult;

/// Evaluate definition `index` of `symbol`'s note, which the model knows as
/// the kind the row names.
pub(crate) type Evaluate = fn(&mut Engine<'_>, &Symbol, usize) -> EvalResult<Value>;
/// Answer a special built-in: it decides for itself what to evaluate.
pub(crate) type Call = fn(&mut Engine<'_>, &Path, Builtin, &[Expr]) -> EvalResult<Value>;
/// Claim the value an expression definition evaluated to for that definition,
/// or hand it back unchanged. `source` is the definition's expression text.
pub(crate) type Adopt = fn(Value, &Symbol, &str) -> Value;

pub(crate) struct Feature {
    /// The definitions this feature evaluates in place of the engine.
    definitions: &'static [(DefinitionKind, Evaluate)],
    /// The special built-ins it answers.
    builtins: &'static [(Builtin, Call)],
    adopt: Option<Adopt>,
}
impl Feature {
    const NONE: Self = Self {
        definitions: &[],
        builtins: &[],
        adopt: None,
    };
}

static FEATURES: &[Feature] = &[
    // Linear plans (`maximize`/`minimize` over a constraint table) and goal
    // seek (`solve(constraint)`), both solved by the `plan` module.
    Feature {
        definitions: &[
            (DefinitionKind::Plan, plans::solve),
            (DefinitionKind::GoalSeek, plans::seek),
        ],
        ..Feature::NONE
    },
    // Tables, and the row `sum(table, row expression)` that walks one.
    Feature {
        definitions: &[(DefinitionKind::Table, tables_impl::evaluate)],
        builtins: &[(Builtin::Sum, tables_impl::call_sum)],
        ..Feature::NONE
    },
    // Timers: a named `countdown(…)`/`stopwatch(…)` learns its definition, so
    // its controls can rewrite it.
    Feature {
        builtins: &[
            (Builtin::Stopwatch, timers::call),
            (Builtin::Countdown, timers::call),
        ],
        adopt: Some(timers::adopt),
        ..Feature::NONE
    },
    // Cached lookups: exchange rates, conversions, forecasts and quotes.
    Feature {
        builtins: &[
            (Builtin::Rate, lookups::call),
            (Builtin::To, lookups::call),
            (Builtin::Forecast, lookups::call),
            (Builtin::ForecastRange, lookups::call),
            (Builtin::Quote, lookups::call),
        ],
        ..Feature::NONE
    },
    // The counts a named checklist heading answers.
    Feature {
        builtins: &[
            (Builtin::Total, checklists::counts),
            (Builtin::Completed, checklists::counts),
            (Builtin::Remaining, checklists::counts),
            (Builtin::Effort, checklists::counts),
        ],
        ..Feature::NONE
    },
];

/// A special built-in no feature claims — `today(x)`, `now(x)`, a
/// wrong-arity `eval`, and the `maximize`/`minimize`/`solve` only a plan or
/// goal seek definition answers — fails the way a checklist question would.
const UNCLAIMED: Call = checklists::counts;

/// The feature that evaluates definitions of `kind`, or `None` for the
/// expressions and literals the engine reads itself.
pub(crate) fn evaluator(kind: DefinitionKind) -> Option<Evaluate> {
    FEATURES
        .iter()
        .flat_map(|feature| feature.definitions)
        .find(|(k, _)| *k == kind)
        .map(|(_, evaluate)| *evaluate)
}

/// What answers a special built-in the engine does not answer itself.
pub(crate) fn call(builtin: Builtin) -> Call {
    FEATURES
        .iter()
        .flat_map(|feature| feature.builtins)
        .find(|(b, _)| *b == builtin)
        .map_or(UNCLAIMED, |(_, call)| *call)
}

/// Let each feature claim a value a named expression definition evaluated to.
pub(crate) fn adopt(value: Value, symbol: &Symbol, source: &str) -> Value {
    FEATURES
        .iter()
        .filter_map(|feature| feature.adopt)
        .fold(value, |value, adopt| adopt(value, symbol, source))
}
