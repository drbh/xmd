//! The registry of feature evaluators: the one list of what each feature
//! answers for the engine. The engine evaluates expressions, names, calls and
//! the core special forms (`if`, `match`, `let`, `eval`, `import`, …); a
//! definition the model knows as a feature ([`DefinitionKind`]) and a special
//! built-in the engine does not answer itself are looked up here, so the
//! engine never names a feature and every feature depends on the engine
//! rather than the other way round. A new feature is one row.
//!
//! Two generic forms sit beside the rows: `clocked`, the module built-in that
//! lets a value that read the clock say for how long that matters, and
//! [`adopt`], which hands a tagged record that asks for it the definition
//! whose call built it.
use crate::engine::{Builtin, Engine, Expr, Parser, Value};
use crate::workspace::Symbol;
use crate::{plans, tables_impl};
use model::{DefinitionKind, Document};
use std::path::Path;
use values::{EvalError, EvalResult, record};

/// Evaluate definition `index` of `symbol`'s note, which the model knows as
/// the kind the row names.
pub(crate) type Evaluate = fn(&mut Engine<'_>, &Symbol, usize) -> EvalResult<Value>;
/// Answer a special built-in: it decides for itself what to evaluate.
pub(crate) type Call = fn(&mut Engine<'_>, &Path, Builtin, &[Expr]) -> EvalResult<Value>;

pub(crate) struct Feature {
    /// The definitions this feature evaluates in place of the engine.
    definitions: &'static [(DefinitionKind, Evaluate)],
    /// The special built-ins it answers.
    builtins: &'static [(Builtin, Call)],
}
impl Feature {
    const NONE: Self = Self {
        definitions: &[],
        builtins: &[],
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
    },
    // A value that read the clock, depending on it only while it says so.
    Feature {
        builtins: &[(Builtin::Clocked, clocked)],
        ..Feature::NONE
    },
    // The checklist counts, the cached lookups (rates, conversions, quotes
    // and forecasts) and the timers are the prelude's
    // (`lang/stdlib/prelude.xmd`), over the records `checklist.tasks` and
    // `cached` read, `tagged` records and `clocked`.
];

/// `clocked(value, ticking)`: `value`, which keeps depending on the clock it
/// read only while `ticking(value)` is true. A running timer reads the clock
/// and moves with it; the same timer once finished reads it too, but stays
/// where it stopped, so nothing need refresh it.
fn clocked(
    engine: &mut Engine<'_>,
    path: &Path,
    builtin: Builtin,
    args: &[Expr],
) -> EvalResult<Value> {
    let [value, ticking] = args else {
        return Err(EvalError::Message(format!(
            "{builtin} expects a value and a function that says whether it still ticks"
        )));
    };
    let before = std::mem::replace(&mut engine.time_dependent, false);
    let result = engine.expr(path, value).and_then(|value| {
        let read = std::mem::replace(&mut engine.time_dependent, false);
        let ticking = engine.expr(path, ticking)?;
        match engine.call(ticking, vec![value.clone()])? {
            Value::Bool(ticks) => Ok((value, read && ticks)),
            _ => Err(EvalError::Message(format!(
                "{builtin}'s function must return true or false"
            ))),
        }
    });
    match result {
        Ok((value, ticks)) => {
            engine.time_dependent = before || ticks;
            Ok(value)
        }
        // A failure may have read the clock: it holds only for that instant.
        Err(error) => {
            engine.time_dependent = true;
            Err(error)
        }
    }
}

/// A special built-in no feature claims — `today(x)`, `now(x)`, a
/// wrong-arity `eval`, and the `maximize`/`minimize`/`solve` only a plan or
/// goal seek definition answers — fails with the words it always has, which
/// date from when the checklist counts answered every unclaimed form.
fn unclaimed(
    _engine: &mut Engine<'_>,
    _path: &Path,
    builtin: Builtin,
    _args: &[Expr],
) -> EvalResult<Value> {
    Err(EvalError::Message(match builtin {
        // Without arguments the engine answers these itself.
        Builtin::Today | Builtin::Now => format!("{builtin} takes no arguments"),
        // A plan or goal seek is a definition, not a value inside one.
        Builtin::Maximize | Builtin::Minimize | Builtin::Solve => {
            format!("{builtin}(...) is only valid as a definition's whole expression")
        }
        // The rest take one argument; here with any other count.
        _ => return Err(EvalError::Arity(builtin)),
    }))
}

const UNCLAIMED: Call = unclaimed;

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

/// The value expression definition `symbol` evaluated to, claimed by that
/// definition when it is a tagged record that asks where it was made
/// ([`values::claim`]) and the definition's whole expression is a call. The
/// record's `origin` becomes `{document, name, line, text, range, function,
/// arguments}`: the definition's note (as a URI), its name, the line it is
/// named on and that line as written, the range of its expression, and the
/// function the expression calls with the source of each argument. Any other
/// value is handed back unchanged.
pub(crate) fn adopt(value: Value, symbol: &Symbol, doc: &Document) -> Value {
    values::claim(&value, || origin(symbol, doc)).unwrap_or(value)
}

/// Where definition `symbol` of `doc` made a value, as [`adopt`] describes
/// it, or `None` when its expression is not a call.
fn origin(symbol: &Symbol, doc: &Document) -> Option<Value> {
    let crate::workspace::SymbolKind::Definition(index) = symbol.kind else {
        return None;
    };
    let definition = &doc.definitions[index];
    let parsed = Parser::parse(&definition.source).ok()?;
    let Expr::Call(function, arguments) = parsed.bare() else {
        return None;
    };
    let raw = definition.value_span.source(doc);
    let leading = raw.len() - raw.trim_start().len();
    let trailing = raw.len() - leading - raw.trim().len();
    let span = common::Span::new(
        definition.value_span.line,
        definition.value_span.start + leading,
        definition.value_span.end - trailing,
    );
    let range = span.range(doc);
    let position = |line: u32, character: u32| {
        record([
            ("line", Value::Count(line as usize)),
            ("character", Value::Count(character as usize)),
        ])
    };
    let line = definition.named.span.line;
    Some(record([
        (
            "document",
            Value::Text(
                common::file_url(&symbol.path)
                    .map(String::from)
                    .unwrap_or_default(),
            ),
        ),
        ("name", Value::Text(definition.named.name.clone())),
        ("line", Value::Count(line)),
        ("text", Value::Text(doc.line(line).into())),
        (
            "range",
            record([
                ("start", position(range.start.line, range.start.character)),
                ("end", position(range.end.line, range.end.character)),
            ]),
        ),
        ("function", Value::Text(function.clone())),
        (
            "arguments",
            Value::list(
                arguments
                    .iter()
                    .map(|argument| {
                        let (start, end) = argument.bounds();
                        Value::Text(definition.source[start..end].trim().into())
                    })
                    .collect(),
            ),
        ),
    ]))
}
