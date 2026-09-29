//! The editing half of the language services, host-independent: what an
//! editor shows and does, and the rendering and agenda the command line uses.
//! [`Request`] lists every service as a method, which is the only way a host
//! reaches them; [`WorkspaceSession`] is the open-buffer state both editing
//! hosts drive. Exposes its interface from the root, as one list of names.
//!
//! One file per feature, grouped by what it is about:
//! - records and queries: `agenda`, over the records and queries in `catalog`;
//! - names and values: `completion`, over the language facts in `analysis`;
//! - how a note looks: `highlighting`, `prose`, `inlays`, `links`, `render`,
//!   and `typing` (the on-type formatting that keeps tables aligned);
//! - what a person can do: `commands` (the actions a host executes),
//!   `code_actions` (offered over a range), `rows` (lenses and row controls).
//!
//! `providers` is the one extension point through which the editor's own
//! features and .x.md feature modules (adapted by `modules`) contribute
//! inlays, hovers, diagnostics, controls and edits.
mod agenda;
mod code_actions;
mod commands;
mod completion;
mod highlighting;
mod inlays;
mod links;
mod modules;
mod prose;
mod providers;
mod render;
mod request;
mod rows;
mod session;
mod typing;

// Every feature is a method on a request; the session is what a host keeps
// open per workspace.
pub use request::Request;
pub use session::{RefreshReport, WorkspaceSession};

// What a host calls without a request: semantic tokens, standalone HTML,
// the day's agenda and on-type formatting.
pub use agenda::today_markdown;
pub use code_actions::TaskToggle;
pub use highlighting::{TOKEN_MODIFIERS, TOKEN_TYPES, semantic_tokens};
pub use render::{fragment, line_classes};
pub use typing::{TRIGGERS, on_type};

// The actions a host executes, and what it can show for them.
pub use commands::{Action, Capabilities, PreparedAction};
