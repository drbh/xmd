//! The language services every host shares, and the source of truth for
//! their behavior: one public interface in front of the private crates beside
//! it. Requests, sessions, queries, rendering and what an editor shows and
//! does, the same on every host: language facts (`analysis`), records and
//! queries (`records`) and the editing features and session (`features`).
//! Hosts reach the services only through here, and every name they can reach
//! is listed, so nothing is exposed by accident. Native input and output is
//! not here: it is the `native` crate, which only the native hosts use.

pub use ::analysis::{
    BUILTINS, Outcome, Signature, definition, flat_symbols, folding_ranges, highlights, references,
    rename, severity_name, signature, symbol_at,
};
pub use ::features::{
    RefreshReport, Request, RowActions, TOKEN_MODIFIERS, TOKEN_TYPES, WorkspaceSession, fragment,
    line_classes, semantic_tokens, today_markdown,
};
pub use ::records::{NoteFiles, Query, Records, display, display_row, lookups};
pub mod commands {
    pub use ::features::{Action, Capabilities, PreparedAction};
}
pub mod hierarchy {
    pub use ::analysis::hierarchy::{decode, dependencies, dependents, prepare, ranges};
}
pub mod typing {
    pub use ::features::TRIGGERS;
}
