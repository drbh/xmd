//! What an editor shows and does, host-independent: each module is one LSP
//! feature over the model and the evaluator. Hosts call these; they never
//! reach into each other's internals.
pub mod actions;
pub mod catalog;
pub mod commands;
pub mod diagnostics;
pub mod hierarchy;
pub mod highlighting;
pub mod inlay_providers;
pub mod inlays;
pub mod intelligence;
pub mod interaction;
pub mod plugin_inlays;
pub mod presentation;
pub mod prose;
pub mod query;
pub mod refactor;
pub mod symbols;
pub mod typing;
