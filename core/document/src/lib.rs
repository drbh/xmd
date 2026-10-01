//! What a note is: the parser and the shapes it produces, and the structures
//! that live inside a note (tables, the definitions a form lays out).
//! Parsing only: the workspace and editing session are runtime state, so
//! both live one layer up in `evaluate`. Exposes its interface from the root.
//!
//! A parse has two layers. `blocks` is generic: what each line is as
//! document structure (heading, list item, table row, prose, fence, comment)
//! and the language's inline forms (definitions, named values, bracket
//! references and calculations, `@key(value)` attributes, links). The
//! feature recognizers, listed in order in `recognizers`, read those blocks
//! and fill the note's features: `attributes` (what a key means, native or
//! declared by a module, and every line that writes one), `sections`,
//! `tasks`, `calculations` (a line of math), `forms` (a definition that calls
//! a form a module declares, and the table under it) and `tables`.
//! `document` is the result and the questions asked of it. `declared` runs
//! the recognizers a module declares as data (a pattern over one kind of
//! block, or whole lines with structure) once the note is parsed; what their
//! matches mean is the declaring module's to say, in .xmd. A note is parsed
//! with the attributes and the forms modules declare, which change how it
//! reads.
mod attributes;
mod blocks;
mod calculations;
mod declared;
mod document;
mod edits;
#[path = "forms.rs"]
mod forms_impl;
mod imports;
mod recognizers;
mod sections;
#[path = "tables.rs"]
mod tables_impl;
mod tasks;

// The parsed note and the pieces named directly by other layers.
pub use attributes::{Attributed, Declaration};
pub use blocks::{Attribute, HighlightKind, Named, Reference, identifier};
pub use document::{DefinitionKind, Document, expression_regions};
pub use edits::{LineIndex, apply_edits, byte_at, utf16};
pub use imports::{ExprImports, is_note_path, note_path};
pub use tasks::TaskState;

// Table and form items stay namespaced: `eval` and the `lang`
// facade name them `document::tables::…` and so on.
// Each file module below is private; the inline `pub mod` here is the
// published namespace, listing exactly the items other layers name.
pub mod recognized {
    pub use crate::declared::{Brush, Group, Match, On, Paint, Rule, Term, Until};
    pub use crate::recognizers::Recognizers;
}
pub mod forms {
    pub use crate::forms_impl::{Column, Form, Formed, Reading, Unknowns, call};
}
pub mod tables {
    pub use crate::blocks::cells;
    pub use crate::tables_impl::{Cell, Domain, Table, grids, scope_at};
}
