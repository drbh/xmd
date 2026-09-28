//! What a note is: the parser and the shapes it produces, and the structures
//! that live inside a note (tables, plans, itineraries). Parsing only:
//! resolving an itinerary's dates and labels runs a module, and the
//! workspace and editing session are runtime state, so both live one layer
//! up in `evaluate`. Exposes its interface from the root.
mod document;
mod imports;
#[path = "itinerary.rs"]
mod itinerary_impl;
#[path = "plans.rs"]
mod plans_impl;
#[path = "tables.rs"]
mod tables_impl;

// The parsed note and the pieces named directly by other layers.
pub use document::{
    Attribute, Document, HighlightKind, Named, Reference, byte_at, expression_regions, identifier,
    utf16,
};
pub use imports::{ExprImports, is_note_path, note_path};

// Table, plan and itinerary parsing each define their own `parse`, so these
// stay namespaced; `eval` (their only consumer) already names them this way.
// Each file module below is private; the inline `pub mod` here is the
// published namespace, listing exactly the items other layers name.
pub mod itinerary {
    pub use crate::itinerary_impl::{Day, KEYS, KINDS, Kind, Stop, clock, month_name};
}
pub mod plans {
    pub use crate::plans_impl::{Constraint, Goal, Plan, goal, regions, seek_body};
}
pub mod tables {
    pub use crate::tables_impl::{
        Cell, Domain, Table, aligned, cells, formatting, grids, line_edit, scope_at,
    };
}
