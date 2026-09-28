//! What an editor shows and does, host-independent. [`Request`] lists every
//! feature as a method, which is the only way a host reaches them. The crate is
//! grouped by what a feature is about: `data` (records and queries), `language`
//! (names and values), `view` (how a note looks) and `controls` (what a person
//! can do). Two modules tie them together: `locate` decides what sits at a
//! position, and `providers` is the one extension point through which the
//! editor's own features and .x.md feature modules contribute inlays, hovers,
//! diagnostics, controls and edits. Exposes its interface from the root.
mod api;
mod controls;
mod data;
mod language;
mod locate;
mod providers;
mod view;

// Editor actions and commands: how a host applies an edit.
pub mod actions {
    pub use crate::controls::code_actions::{TaskToggle, apply_edits};
}
pub mod commands {
    pub use crate::controls::commands::{Action, Capabilities, PreparedAction};
}
// The day's agenda, rendered as markdown.
pub mod agenda {
    pub use crate::data::agenda::today_markdown;
}
// Diagnostics and the dependency graph they can point across.
pub mod diagnostics {
    pub use crate::language::diagnostics::severity_name;
}
pub mod hierarchy {
    pub use crate::language::hierarchy::{decode, dependencies, dependents, prepare, ranges};
}
// Symbol lookup and go-to for a position in a document.
pub mod intelligence {
    pub use crate::language::navigation::occurrences;
    pub use crate::language::signature::signature;
    pub use crate::locate::symbol_at;
}
// Semantic tokens and their legend.
pub mod presentation {
    pub use crate::view::presentation::{TOKEN_MODIFIERS, TOKEN_TYPES, semantic_tokens};
}
// The `wtf query` language and how it renders.
pub mod query {
    pub use crate::data::catalog::value::display;
    pub use crate::data::query::{Query, QueryResult};
}
// Standalone HTML rendering, shared by the CLI's export and the browser.
pub mod rendering {
    pub use crate::view::rendering::{fragment, line_classes};
}
// Every feature is a method on a request.
pub use api::Request;
// The editing session a host keeps open per workspace.
pub mod session {
    pub use crate::api::{RefreshReport, WorkspaceSession};
}
// The document outline and folding ranges.
pub mod symbols {
    pub use crate::language::symbols::{flat_symbols, folding_ranges};
}
// On-type formatting triggers.
pub mod typing {
    pub use crate::view::typing::{TRIGGERS, on_type};
}
