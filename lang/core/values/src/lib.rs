//! What a note's values are, apart from the engine that computes them: the
//! value kinds and their display and JSON forms, the operators and pure
//! built-ins over them, the failure vocabulary every evaluation answers with,
//! the lookup cache's keys, tagged records, and the records that carry
//! values across the module boundary. None of it evaluates a note, so it sits below both the module
//! compiler and the evaluator, and each can be read without the other.
//!
//! Exposes its interface from the root, as one flat list.
mod arithmetic;
mod collection;
mod error;
mod functional;
mod lookups;
mod records;
mod solver;
mod tagged;
mod value;

// The failure vocabulary every evaluation answers with.
pub use error::{CurrencyOp, Depth, EvalError, EvalResult, Limit, Overflow, UnitOp};
// The value kinds, the host objects a value may hold, and their JSON forms.
pub use value::{
    CHECKLIST_TASKS, Captured, Function, HostObject, Measured, Namespace, TaskKey, Unit, Value,
    date_value, json, literal, next_occurrence, optional, record, value_json,
};
// What operators and pure built-ins compute over values.
pub use arithmetic::binary;
pub use functional::{Size, builtin, check_size, check_size_within, compare, sum};
pub use solver::LINEAR_COMPARISON;
// Values crossing into and out of modules as records.
pub use records::{Fields, FromValue, RecordFields, ToValue, from_json, geometry, list};
// Fetched lookup values, by kind and key; what they mean is the prelude's.
pub use lookups::{Lookup, LookupKey, Store};
// The named sets of workspace records a query or module binds.
pub use collection::Collection;
pub use tagged::{claim, claimed};
