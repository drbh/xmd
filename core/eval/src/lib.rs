//! What a note computes: the evaluator and the objects only it builds
//! (resources, tables). The value kinds, the failure
//! vocabulary and cached lookups it answers with live one layer down in
//! `values`. The text charts and the glyph vocabulary every label draws from
//! live in the stdlib's `format` module.
//!
//! The engine (`engine`, `calls`, `linear`) evaluates expressions and the
//! core special forms, and reaches every feature through `features`, the
//! registry of feature evaluators: `forms` (a definition that calls a form a
//! module declares: its expressions read as linear forms, the module's
//! `define` hook saying what it is worth) and `tables` each register the
//! definitions and built-ins they answer, and depend on the engine rather
//! than the engine on them. `lookups` is the `cached` built-in's read of the
//! lookup cache.
//! `checklists` is a checklist's tasks as records, which the prelude's counts
//! read, and `prelude` is how a name nothing else defines reaches the prelude
//! library.
//!
//! `context` holds the one workspace snapshot, clock and memo a request
//! evaluates against; `memo` is that memo, and what of it an owner may keep
//! for later requests. The .xmd modules, their registry and the link-module
//! contract are described in `modules`; `module_runtime` is how the
//! evaluator compiles and calls them.
//!
//! Exposes its interface from the root, as one flat list of the names only
//! `eval` owns. What it reads from `values`, `modules`, `syntax`, `document` and
//! `common` is not re-exported: the `lang` facade takes those from their own
//! crates.
mod calls;
mod checklists;
mod context;
mod contract;
mod engine;
mod features;
mod forms;
mod host;
mod imports;
mod linear;
mod lookups;
mod memo;
mod module_runtime;
mod prelude;
mod resources;
#[path = "tables.rs"]
mod tables_impl;
mod workspace;

// The request context every feature evaluates against, and its clock.
pub use context::{Clock, RequestContext};
// What an owner keeps of evaluation across requests, and how a caller learns
// that module code read the clock.
pub use memo::{Evaluations, reads_clock};
// The note graph: parsed documents, resolved by path and name.
pub use workspace::{Symbol, SymbolKind, Workspace};
// An import's target member, resolved against the note that defines it.
pub use imports::member_symbol;
// The functions the prelude library gives every note by name.
pub use prelude::PreludeFunction;

// The evaluator and the values it produces.
pub use engine::{Bindings, Engine};
// A lookup evaluation read, and the note text it was read for.
pub use host::HostPresenting;
pub use lookups::LookupRead;
// The objects only the evaluator builds, as a `Value::Host` holds them.
pub use tables_impl::TableValue;
// What a form's module says about a definition besides its value.
pub use forms::About;
// How a registry of .xmd modules compiles, which only the evaluator can do.
pub use module_runtime::CompileModules;
// A resource value and how a hover presents it.
pub use resources::{Resource, ResourcePresenting};

// Table names that read as generic on their own (`origin`, `table`) stay
// under a namespace. A namespace cannot share its name with a file module, so
// that file sits under `_impl`.
pub mod tables {
    pub use crate::tables_impl::{
        literal_value, origin, resolve_reference, table, validate_rename,
    };
}
/// The contract with the standard library: every stdlib function native code
/// calls, declared in `CONTRACT` and called only through the typed functions
/// in each module's namespace.
pub mod stdlib {
    pub use crate::contract::{
        CONTRACT, Contract, Presented, Role, format, modules, prelude, resource, shown, task, today,
    };
}
