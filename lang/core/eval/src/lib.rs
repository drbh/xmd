//! What a note computes: the evaluator and the objects only it builds
//! (timers, resources, plans, tables). The value kinds, the failure
//! vocabulary and cached lookups it answers with live one layer down in
//! `values`. The text charts and the glyph vocabulary every label draws from
//! live in the stdlib's `format` module.
//!
//! `context` holds the one workspace snapshot, clock and memo a request
//! evaluates against. The .xmd modules, their registry and the link-module
//! contract are described in `modules`; `module_runtime` is how the
//! evaluator compiles and calls them. `itinerary` resolves the days and
//! stops `model::itinerary` parsed by calling the `itinerary_core` module.
//!
//! Exposes its interface from the root, as one flat list of the names only
//! `eval` owns. What it reads from `values`, `modules`, `syntax`, `model` and
//! `common` is not re-exported: the `lang` facade takes those from their own
//! crates.
mod calls;
mod context;
mod engine;
mod host;
mod imports;
#[path = "itinerary.rs"]
mod itinerary_impl;
mod linear;
mod module_runtime;
mod plans;
mod resources;
#[path = "tables.rs"]
mod tables_impl;
mod timers;
mod workspace;

// The request context every feature evaluates against, and its clock.
pub use context::{Clock, RequestContext};
// The note graph: parsed documents, resolved by path and name.
pub use workspace::{Symbol, SymbolKind, Workspace};
// An import's target member, resolved against the note that defines it.
pub use imports::member_symbol;

// The evaluator and the values it produces.
pub use engine::{Bindings, Engine};
pub use host::HostPresenting;
// The objects only the evaluator builds, as a `Value::Host` holds them.
pub use plans::PlanValue;
pub use tables_impl::TableValue;
// How a registry of .xmd modules compiles, which only the evaluator can do.
pub use module_runtime::CompileModules;
// A resource value and how a hover presents it.
pub use resources::{Resource, ResourcePresenting};
// Timer edits a command line or editor action can apply.
pub use timers::{Timer, TimerAction, edit_timer};

// Table and itinerary names that read as generic on their own (`origin`,
// `table`, `dates`, `label`) stay under a namespace. A namespace cannot share
// its name with a file module, so those two files sit under `_impl`.
pub mod itinerary {
    pub use crate::itinerary_impl::{dates, day_record, display_time, label, try_dates};
}
pub mod tables {
    pub use crate::tables_impl::{
        literal_value, origin, resolve_reference, table, validate_rename,
    };
}
