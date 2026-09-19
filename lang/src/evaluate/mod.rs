//! What a note computes: the evaluator and the value kinds it produces
//! (timers, resources, lookups, plans), plus the text charts and the glyph
//! vocabulary every label draws from.
//!
//! `modules` compiles the .wtf feature, link and library modules and `context`
//! holds the one workspace snapshot, clock and memo a request evaluates
//! against. `link_features` is the contract a link module implements, next to
//! the module compiler that produces it. `itinerary` resolves the days and
//! stops `model::itinerary` parsed by calling the `itinerary_core` module.
pub mod charts;
pub mod context;
pub mod engine;
pub mod feeds;
pub mod functional;
pub mod github;
pub mod glyphs;
pub mod itinerary;
pub mod link_features;
pub mod lookups;
pub mod modules;
pub mod plans;
pub mod resources;
pub mod timers;

pub mod solver;
