//! What can go wrong while a note computes, as a value instead of a sentence.
//!
//! Every message the evaluator can produce is a variant here, and `Display`
//! writes exactly the text hosts used to build by hand. Consumers that need to
//! reason about a failure — is this note wrong, or is its data simply not
//! fetched yet? — match on the variant instead of reading the words, and
//! consumers that only render it let `?` turn it into its `String` at the
//! boundary.
use crate::lookups::LookupKey;
use crate::value::Unit;
use common::{Currency, ValueType};
use syntax::{BinaryOp, Builtin};

pub type EvalResult<T> = Result<T, EvalError>;

/// Which chain ran too deep.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::Display)]
pub enum Depth {
    /// Named definitions depending on one another.
    #[strum(to_string = "Dependency chain exceeds 64 levels")]
    Dependency,
    /// Nested function calls.
    #[strum(to_string = "Function call depth exceeds 32")]
    Call,
    /// `@after` chains between tasks.
    #[strum(to_string = "Task dependency chain is too deep")]
    Task,
}
/// A size ceiling a value walked into.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::Display)]
pub enum Limit {
    #[strum(to_string = "Text exceeds 1 MiB")]
    Text,
    #[strum(to_string = "List exceeds 4096 items")]
    ListItems,
    #[strum(to_string = "List exceeds the collection size limit")]
    Collection,
    #[strum(to_string = "Value exceeds the collection or text size limit")]
    Value,
    #[strum(to_string = "Padding exceeds the size limit")]
    Padding,
    #[strum(to_string = "Repeated text exceeds the size limit")]
    RepeatedText,
}
/// A quantity that ran out of range.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::Display)]
pub enum Overflow {
    #[strum(to_string = "Duration overflow")]
    Duration,
    #[strum(to_string = "Date overflow")]
    Date,
    #[strum(to_string = "Date/time overflow")]
    DateTime,
    #[strum(to_string = "Number overflow")]
    Number,
    #[strum(to_string = "Recurrence date overflow")]
    Recurrence,
}
/// What was being attempted when two currencies failed to meet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurrencyOp {
    Compare,
    Combine,
    Add,
}
/// What was being attempted when two plan units failed to meet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnitOp {
    Add,
    AddScaled,
    Multiply,
    Divide,
    Compare,
}
/// The kinds of value that answer for their own property names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, strum::Display)]
#[strum(serialize_all = "snake_case")]
pub enum PropertyOwner {
    Forecast,
    Plan,
    Resource,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EvalError {
    // Names.
    UnknownName {
        name: String,
    },
    AmbiguousName {
        name: String,
    },
    UnknownFunction(String),
    UnknownColumn {
        name: String,
        table: String,
    },
    NotATable(String),
    /// A decision column read outside, and then inside, a plan.
    DecisionColumnOutsidePlan(String),
    DecisionColumnBareTable(String),

    // Cycles and budgets.
    Cycle {
        names: Vec<String>,
    },
    CycleThrough {
        name: String,
    },
    TaskCycle {
        names: Vec<String>,
    },
    DepthExceeded(Depth),
    StepLimit,
    LimitExceeded(Limit),
    Overflowed(Overflow),
    DivisionByZero,
    /// Neither operand is a number, money, ratio, count or duration.
    UnsupportedArithmetic,

    // Types.
    /// A binary operation's failure, decorated with the operand types.
    Binary {
        op: BinaryOp,
        left: ValueType,
        right: ValueType,
        source: Box<EvalError>,
    },
    CurrencyMismatch {
        op: CurrencyOp,
        left: Currency,
        right: Currency,
    },
    UnitMismatch {
        op: UnitOp,
        left: Unit,
        right: Unit,
    },
    UnknownField {
        key: String,
        on: Option<ValueType>,
    },
    UnknownProperty {
        owner: PropertyOwner,
        name: String,
    },
    /// "Expected a function", "Expected text", … The payload is the tail.
    Expected(&'static str),
    Arity(Builtin),
    FunctionArity {
        expected: usize,
        found: usize,
    },

    // Data that has to be fetched before a note can answer.
    NotCached(LookupKey),
    Unreadable(LookupKey),
    /// A cached lookup that answered with a problem of its own.
    Lookup {
        key: LookupKey,
        source: Box<EvalError>,
    },

    // Modules.
    Module {
        id: String,
        hook: String,
        source: Box<EvalError>,
    },
    ModuleUnavailable(String),
    ModuleDisabled(String),
    UnknownImport(String),
    /// `import` reached a module that is not a library. Only libraries are
    /// plain functions a note may name; link and feature modules are called by
    /// the host, never from a note.
    NotALibrary {
        id: String,
        /// The module kind's own name, as `ModuleKind` spells it.
        kind: &'static str,
    },
    /// `import(id).name` reached for a member the library does not export:
    /// a `_` name, a name left out of its `exports`, or no such name at all.
    NotExported {
        id: String,
        name: String,
    },

    /// Text from outside the evaluator: `error("…")`, a module hook's own
    /// message, or the error field of a cached lookup. Opaque, so whether it
    /// describes unfetched data is still decided by reading it.
    Custom(String),
    /// Parser and lexer text, or a one-off message that has only ever been
    /// produced from a single site.
    Message(String),
}

/// The legacy reading of an opaque message: does it describe data that has not
/// been fetched yet, rather than a mistake in the note? Only text the evaluator
/// did not write is still classified this way.
fn describes_pending_data(message: &str) -> bool {
    message.starts_with("No cached")
        || message.contains("; run xmd refresh")
        || message.contains("no forecast yet")
}

impl EvalError {
    /// Text the evaluator did not write, somewhere in this failure.
    fn carries_opaque_text(&self) -> bool {
        match self {
            Self::Custom(_) => true,
            Self::Binary { source, .. }
            | Self::Lookup { source, .. }
            | Self::Module { source, .. } => source.carries_opaque_text(),
            _ => false,
        }
    }
    /// Unfetched data is a state, not a mistake: hosts report these as warnings.
    pub fn is_pending(&self) -> bool {
        // A message that came from outside can only be read, so the whole
        // sentence — attribution and all — is what gets classified.
        if self.carries_opaque_text() {
            return describes_pending_data(&self.to_string());
        }
        match self {
            Self::NotCached(_) => true,
            Self::Binary { source, .. }
            | Self::Lookup { source, .. }
            | Self::Module { source, .. } => source.is_pending(),
            _ => false,
        }
    }
    /// A failure that names the definitions it walked through.
    pub fn is_cycle(&self) -> bool {
        matches!(self, Self::Cycle { .. } | Self::TaskCycle { .. })
    }
    /// Decorate a binary operation's failure with the operand types.
    pub fn in_binary(self, op: BinaryOp, left: ValueType, right: ValueType) -> Self {
        Self::Binary {
            op,
            left,
            right,
            source: Box::new(self),
        }
    }
    /// Attribute a failure to the module hook it came out of.
    pub fn in_module(self, id: &str, hook: &str) -> Self {
        Self::Module {
            id: id.to_string(),
            hook: hook.to_string(),
            source: Box::new(self),
        }
    }
    /// Attribute a failure to the lookup whose cached value produced it.
    pub fn in_lookup(self, key: LookupKey) -> Self {
        Self::Lookup {
            key,
            source: Box::new(self),
        }
    }
}

fn names(names: &[String]) -> String {
    names.join(" → ")
}

/// A lookup named mid-sentence: "rate USD→EUR", "quote for NVDA", …
struct Phrase<'a>(&'a LookupKey);
impl std::fmt::Display for Phrase<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            LookupKey::Rate { from, to } => write!(f, "rate {from}→{to}"),
            LookupKey::Quote(symbol) => write!(f, "quote for {symbol}"),
            LookupKey::Forecast { place, date } => write!(f, "forecast for {place} on {date}"),
        }
    }
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownName { name } => write!(f, "Unknown name '{name}'"),
            Self::AmbiguousName { name } => {
                write!(
                    f,
                    "Ambiguous name '{name}'; use a unique name within this note"
                )
            }
            Self::UnknownFunction(name) => write!(f, "Unknown function '{name}'"),
            Self::UnknownColumn { name, table } => {
                write!(f, "Unknown column '{name}' in table '{table}'")
            }
            Self::NotATable(name) => write!(f, "'{name}' is not a table"),
            Self::DecisionColumnOutsidePlan(name) => write!(
                f,
                "'{name}' is a decision column; a plan chooses it, so sum over it inside maximize or minimize"
            ),
            Self::DecisionColumnBareTable(name) => {
                write!(f, "'{name}' is a decision column; use it inside a plan")
            }
            Self::Cycle { names: chain } => write!(f, "Dependency cycle: {}", names(chain)),
            Self::CycleThrough { name } => write!(f, "Dependency cycle through {name}"),
            Self::TaskCycle { names: chain } => {
                write!(f, "Task dependency cycle: {}", names(chain))
            }
            Self::DepthExceeded(depth) => write!(f, "{depth}"),
            Self::StepLimit => {
                f.write_str("Evaluation exceeds 200,000 steps; simplify nested row calculations")
            }
            Self::LimitExceeded(limit) => write!(f, "{limit}"),
            Self::Overflowed(overflow) => write!(f, "{overflow}"),
            Self::DivisionByZero => f.write_str("Division by zero"),
            Self::UnsupportedArithmetic => f.write_str("Unsupported arithmetic types"),
            Self::Binary {
                op,
                left,
                right,
                source,
            } => write!(f, "{source} ({left} {op} {right})"),
            Self::CurrencyMismatch { op, left, right } => {
                let (verb, joint) = match op {
                    CurrencyOp::Compare => ("compare", "with"),
                    CurrencyOp::Combine => ("combine", "and"),
                    CurrencyOp::Add => ("add", "and"),
                };
                write!(
                    f,
                    "Cannot {verb} {left} {joint} {right}; convert with to(value, {right})"
                )
            }
            Self::UnitMismatch { op, left, right } => {
                let (verb, joint) = match op {
                    UnitOp::Add => ("add", "and"),
                    UnitOp::AddScaled => ("add terms scaled by", "and"),
                    UnitOp::Multiply => ("multiply", "by"),
                    UnitOp::Divide => ("divide", "by"),
                    UnitOp::Compare => ("compare", "with"),
                };
                write!(f, "Cannot {verb} {left} {joint} {right}")
            }
            Self::UnknownField { key, on: None } => write!(f, "Unknown field '{key}'"),
            Self::UnknownField { key, on: Some(ty) } => {
                write!(f, "Unknown field '{key}' on {ty}")
            }
            Self::UnknownProperty { owner, name } => {
                write!(f, "Unknown {owner} property '{name}'")
            }
            Self::Expected(what) => write!(f, "Expected {what}"),
            Self::Arity(function) => write!(f, "{function} expects one argument"),
            Self::FunctionArity { expected, found } => {
                write!(f, "Function expects {expected} arguments, got {found}")
            }
            Self::NotCached(key) => write!(
                f,
                "No cached {}; run xmd refresh or use the ⟳ lookups lens",
                Phrase(key)
            ),
            Self::Unreadable(key) => write!(f, "Cached {} is unreadable", Phrase(key)),
            Self::Lookup { key, source } => match key {
                LookupKey::Rate { from, to } => write!(f, "Rate {from}→{to}: {source}"),
                LookupKey::Quote(symbol) => write!(f, "Quote {symbol}: {source}"),
                LookupKey::Forecast { place, date } => {
                    write!(f, "Forecast for {place} on {date}: {source}")
                }
            },
            Self::Module { id, hook, source } => write!(f, "Module {id}.{hook}: {source}"),
            Self::ModuleUnavailable(id) => write!(f, "Module '{id}' is unavailable or disabled"),
            Self::ModuleDisabled(id) => write!(f, "Module '{id}' is disabled"),
            Self::UnknownImport(id) => write!(f, "Unknown or undeclared import '{id}'"),
            Self::NotALibrary {
                id,
                kind: "library",
            } => {
                write!(
                    f,
                    "Module '{id}' exports nothing; the engine calls it by name"
                )
            }
            Self::NotALibrary { id, kind } => write!(
                f,
                "Module '{id}' is a {kind} module and cannot be imported; only libraries can"
            ),
            Self::NotExported { id, name } => {
                write!(f, "'{name}' is not exported by module '{id}'")
            }
            Self::Custom(message) | Self::Message(message) => f.write_str(message),
        }
    }
}

impl From<EvalError> for String {
    fn from(error: EvalError) -> Self {
        error.to_string()
    }
}
impl From<String> for EvalError {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}
impl From<&str> for EvalError {
    fn from(message: &str) -> Self {
        Self::Message(message.to_string())
    }
}
