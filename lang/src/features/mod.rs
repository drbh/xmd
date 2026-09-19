//! What an editor shows and does, host-independent: each module here is one
//! editor feature over the model and the evaluator. `session` lists them all as
//! methods on a request, which is the only way a host reaches them; the feature
//! modules never reach into each other's internals.
pub mod actions;
pub mod agenda;
pub mod catalog;
pub mod commands;
pub mod completion;
pub mod diagnostics;
pub mod hierarchy;
pub mod highlighting;
pub mod hover;
pub mod inlays;
pub mod inspection;
pub mod intelligence;
pub mod interaction;
pub mod modules;
pub mod presentation;
pub mod prose;
pub mod query;
pub mod refactor;
pub mod rendering;
pub mod session;
pub mod signature;
pub mod symbols;
pub mod typing;
