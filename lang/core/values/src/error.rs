//! What can go wrong while a note computes, as a value instead of a sentence.
//!
//! Every message the evaluator can produce is a variant here, and `Display`
//! writes exactly the text hosts used to build by hand. Consumers that need to
//! reason about a failure — is this note wrong, or is its data simply not
//! fetched yet? — match on the variant instead of reading the words, and
//! consumers that only render it let `?` turn it into its `String` at the
//! boundary.
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
        left: String,
        right: String,
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
    /// A field an object that answers for its own property names (a plan,
    /// a resource, a tagged record) does not have.
    UnknownProperty {
        owner: ValueType,
        name: String,
    },
    /// "Expected a function", "Expected text", … The payload is the tail.
    Expected(&'static str),
    Arity(Builtin),
    FunctionArity {
        expected: usize,
        found: usize,
    },
    /// A function called by name with too few or too many arguments: it
    /// takes `required` to `params` of them.
    CallArity {
        name: String,
        required: usize,
        params: usize,
        found: usize,
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

    /// Text from outside the evaluator: `error("…")` or a module hook's own
    /// message.
    Custom(String),
    /// `pending("…")`: data the note reads has not been fetched yet, such as
    /// a lookup nothing has cached. A state of the world rather than a mistake
    /// in the note, so hosts report it as a warning.
    Pending(String),
    /// Parser and lexer text, or a one-off message that has only ever been
    /// produced from a single site.
    Message(String),
}

impl EvalError {
    /// Unfetched data is a state, not a mistake: hosts report these as warnings.
    pub fn is_pending(&self) -> bool {
        match self {
            Self::Pending(_) => true,
            Self::Binary { source, .. } | Self::Module { source, .. } => source.is_pending(),
            _ => false,
        }
    }
    /// A failure that names the definitions it walked through.
    pub fn is_cycle(&self) -> bool {
        matches!(self, Self::Cycle { .. } | Self::TaskCycle { .. })
    }
    /// Decorate a binary operation's failure with the operand types.
    pub fn in_binary(self, op: BinaryOp, left: &str, right: &str) -> Self {
        Self::Binary {
            op,
            left: left.into(),
            right: right.into(),
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
}

fn names(names: &[String]) -> String {
    names.join(" → ")
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
                write!(
                    f,
                    "Unknown {} property '{name}'",
                    owner.as_str().to_lowercase()
                )
            }
            Self::Expected(what) => write!(f, "Expected {what}"),
            Self::Arity(function) => write!(f, "{function} expects one argument"),
            Self::FunctionArity { expected, found } => {
                write!(f, "Function expects {expected} arguments, got {found}")
            }
            Self::CallArity {
                name,
                required: 1,
                params: 1,
                ..
            } => write!(f, "{name} expects one argument"),
            Self::CallArity {
                name,
                required,
                params,
                found,
            } if required == params => write!(f, "{name} expects {params} arguments, got {found}"),
            Self::CallArity {
                name,
                required,
                params,
                found,
            } => write!(
                f,
                "{name} expects {required} to {params} arguments, got {found}"
            ),
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
            Self::Custom(message) | Self::Pending(message) | Self::Message(message) => {
                f.write_str(message)
            }
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
