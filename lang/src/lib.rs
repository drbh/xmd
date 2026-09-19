//! WTF, the written text format. The crate is layered from the bottom up:
//! `model` parses notes, `evaluate` computes their values, `features` turn
//! both into editor behavior, and `hosts` deliver the features over a
//! transport. Every module inside a layer is a sibling of the others.
//!
//! A layer only calls downwards, so a subject can span two of them:
//! `model::itinerary` parses days and stops, and `evaluate::itinerary`
//! resolves their dates and labels through the `itinerary_core` module.
//! Feature modules written in the .wtf language
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
    charts, context, engine, error, feeds, github, glyphs, link_features, lookups, modules, plans,
    resources, timers,
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
pub use model::{document, paths, tables, workspace};

// An itinerary is parsed in `model` and resolved in `evaluate`; both halves
// answer to `wtf::itinerary`.
pub mod itinerary {
    pub use crate::evaluate::itinerary::*;
    pub use crate::model::itinerary::*;
}

pub use context::RequestContext;
