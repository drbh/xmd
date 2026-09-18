//! WTF, the written text format. The crate is layered from the bottom up:
//! `model` parses notes, `evaluate` computes their values, `features` turn
//! both into editor behavior, and `hosts` deliver the features over a
//! transport. Every module inside a layer is a sibling of the others.
pub mod evaluate;
pub mod features;
pub mod hosts;
pub mod model;

// Every leaf module is also reachable at the crate root, so `wtf::engine`
// and `wtf::document` name the same things the layers do.
pub use evaluate::{charts, engine, glyphs, lookups, plans, resources, timers};
pub use features::{
    actions, catalog, diagnostics, hierarchy, highlighting, intelligence, interaction,
    presentation, prose, query, refactor, symbols, typing,
};
#[cfg(feature = "browser")]
pub use hosts::browser;
#[cfg(feature = "native")]
pub use hosts::{cli, editor};
pub use model::{document, itinerary, paths, tables, workspace};
