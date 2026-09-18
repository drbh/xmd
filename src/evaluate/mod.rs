//! What a note computes: the evaluator and the value kinds it produces
//! (timers, resources, lookups, plans), plus the text charts and the glyph
//! vocabulary every label draws from.
pub mod charts;
pub mod context;
pub mod engine;
pub mod functional;
pub mod github;
pub mod glyphs;
pub mod link_features;
pub mod lookups;
pub mod plans;
pub mod plugins;
pub mod resources;
pub mod timers;

pub mod solver;
