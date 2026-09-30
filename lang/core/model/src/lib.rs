//! What a note is: the parser and the shapes it produces, and the structures
//! that live inside a note (tables, plans, itineraries). Parsing only:
//! resolving an itinerary's dates and labels runs a module, and the
//! workspace and editing session are runtime state, so both live one layer
//! up in `evaluate`. Exposes its interface from the root.
//!
//! A parse has two layers. `blocks` is generic: what each line is as
//! document structure (heading, list item, table row, prose, fence, comment)
//! and the language's inline forms (definitions, named values, bracket
//! references and calculations, `@key(value)` attributes, links). The
//! feature recognizers, listed in order in `recognizers`, read those blocks
//! and fill the note's features: `attributes` (what a key means), `sections`,
//! `tasks`, `events`, `calculations` (a line of math), `plans`, `tables` and
//! `itinerary`. `document` is the result and the questions asked of it.
//! `declared` runs the recognizers a module declares as data (a pattern over
//! one kind of block) once the note is parsed.
mod attributes;
mod blocks;
mod calculations;
mod declared;
mod document;
mod edits;
mod events;
mod imports;
#[path = "itinerary.rs"]
mod itinerary_impl;
#[path = "plans.rs"]
mod plans_impl;
mod recognizers;
mod sections;
#[path = "tables.rs"]
mod tables_impl;
mod tasks;

// The parsed note and the pieces named directly by other layers.
pub use blocks::{Attribute, HighlightKind, Named, Reference, identifier};
pub use document::{DefinitionKind, Document, expression_regions};
pub use edits::{LineIndex, apply_edits, byte_at, end_position, utf16};
pub use imports::{ExprImports, is_note_path, note_path};
pub use tasks::TaskState;

// Table, plan and itinerary items stay namespaced: `eval` and the `lang`
// facade name them `model::tables::…` and so on.
// Each file module below is private; the inline `pub mod` here is the
// published namespace, listing exactly the items other layers name.
pub mod recognized {
    pub use crate::declared::{Group, MAX_MATCHES, Match, On, Paint, Rule};
}
pub mod itinerary {
    pub use crate::itinerary_impl::{Day, KEYS, KINDS, Kind, Stop, clock, month_name};
}
pub mod plans {
    pub use crate::plans_impl::{Constraint, Goal, Plan, goal, regions, seek_body};
}
pub mod tables {
    pub use crate::blocks::cells;
    pub use crate::tables_impl::{
        Cell, Domain, Table, aligned, formatting, grids, line_edit, scope_at,
    };
}
