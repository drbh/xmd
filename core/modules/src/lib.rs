//! The .xmd modules as data: what a feature, link, library, command or
//! provider module declares, how its source compiles and links into a
//! registry, and the contract a link module fulfils. Running a module's code
//! is the evaluator's job, reached through `ModuleEnvironment`, so this crate
//! describes and validates modules without naming the engine, and the engine
//! calls into them without owning their shape.
//!
//! Exposes its interface from the root, as one flat list.
mod link_features;
mod module;
mod registry;

// A compiled module, its kind and hooks, and the environment and clock it
// runs in.
pub use module::{
    Declared, Evaluator, Hook, Joins, Module, ModuleEnvironment, ModuleKind, NewEnvironment,
    has_clock, is_module_path, no_clock,
};
// What the host hands each hook and each `step` loop and what must come back,
// declared as data.
pub use module::{Effect, HOOK_RECORDS, HOOKS, HookContract, HookRecord, STEPS, StepProtocol};
// The compiled set of modules, linked and ready to call.
pub use registry::ModuleRegistry;
// The link modules a request consults, and the refresh formats they name.
pub use link_features::{Cache, LinkFeatures, Metadata, RefreshFormat, RefreshRequest};
