//! WTF, the written text format. The crate is layered from the bottom up:
//! `model` parses notes, `evaluate` computes their values, `features` turn
//! both into editor behavior, and `hosts` deliver the features over a
//! transport. Every module inside a layer is a sibling of the others.
//!
//! A layer only calls downwards. Feature modules written in the .wtf language
//! are the one path back up: the engine evaluates them like any other note, and
//! `features::modules` adapts their hooks into inlays, hovers, diagnostics,
//! formatting and actions. Hosts see none of that; they build a
//! [`RequestContext`] and call the methods `features::session` defines on it.
pub mod evaluate;
pub mod features;
pub mod hosts;
pub mod model;

// Every leaf module is also reachable at the crate root, so `wtf::engine`
// and `wtf::document` name the same things the layers do.
pub use evaluate::{
    charts, context, engine, github, glyphs, link_features, lookups, modules, plans, resources,
    timers,
};
// `modules` at the root is the evaluator's registry; the feature adapter over
// its hooks stays reachable as `wtf::features::modules`.
pub use features::{
    actions, catalog, commands, completion, diagnostics, hierarchy, highlighting, hover, inlays,
    intelligence, interaction, presentation, prose, query, refactor, rendering, session, signature,
    symbols, typing,
};
#[cfg(feature = "browser")]
pub use hosts::browser;
#[cfg(feature = "native")]
pub use hosts::{cli, editor};
pub use model::{document, itinerary, paths, tables, workspace};

pub use context::RequestContext;
