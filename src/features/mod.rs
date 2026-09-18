//! What an editor shows and does, host-independent: each module is one LSP
//! feature over the model and the evaluator. Hosts call these; they never
//! reach into each other's internals.
pub mod actions;
pub mod catalog;
pub mod diagnostics;
pub mod hierarchy;
pub mod highlighting;
pub mod intelligence;
pub mod interaction;
pub mod presentation;
pub mod prose;
pub mod query;
pub mod refactor;
pub mod symbols;
pub mod typing;
