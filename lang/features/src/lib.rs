//! What an editor shows and does, host-independent: each module here is one
//! editor feature over the model and the evaluator. `session` lists them all as
//! methods on a request, which is the only way a host reaches them; the feature
//! modules never reach into each other's internals. Exposes its interface from
//! the root.
#[path = "actions.rs"]
mod actions_impl;
#[path = "agenda.rs"]
mod agenda_impl;
mod catalog;
#[path = "commands.rs"]
mod commands_impl;
mod completion;
#[path = "diagnostics.rs"]
mod diagnostics_impl;
#[path = "hierarchy.rs"]
mod hierarchy_impl;
mod highlighting;
mod hover;
mod inlays;
mod inspection;
#[path = "intelligence.rs"]
mod intelligence_impl;
mod interaction;
mod modules;
#[path = "presentation.rs"]
mod presentation_impl;
mod prose;
#[path = "query.rs"]
mod query_impl;
mod refactor;
#[path = "reference.rs"]
mod reference_impl;
#[path = "rendering.rs"]
mod rendering_impl;
#[path = "session.rs"]
mod session_impl;
mod signature;
#[path = "symbols.rs"]
mod symbols_impl;
#[path = "typing.rs"]
mod typing_impl;

// Each `#[path]` module above keeps its file private (`..._impl`); the inline
// `pub mod` below it is the one published namespace at that name, re-exporting
// only what hosts and the facade actually name (`features::<namespace>::…`).

// Editor actions and commands: how a host applies an edit.
pub mod actions {
    pub use crate::actions_impl::{TaskToggle, apply_edits};
}
pub mod commands {
    pub use crate::commands_impl::{Action, Capabilities, PreparedAction};
}
// The day's agenda, rendered as markdown.
pub mod agenda {
    pub use crate::agenda_impl::today_markdown;
}
// Diagnostics and the dependency graph they can point across.
pub mod diagnostics {
    pub use crate::diagnostics_impl::severity_name;
}
pub mod hierarchy {
    pub use crate::hierarchy_impl::{decode, dependencies, dependents, prepare, ranges};
}
// Symbol lookup and go-to for a position in a document.
pub mod intelligence {
    pub use crate::intelligence_impl::{occurrences, signature, symbol_at};
}
// Semantic tokens and their legend.
pub mod presentation {
    pub use crate::presentation_impl::{TOKEN_MODIFIERS, TOKEN_TYPES, semantic_tokens};
}
// The `wtf query` language and how it renders.
pub mod query {
    pub use crate::query_impl::{Query, QueryValue};
}
// The reference model the CLI and the browser host both render from.
pub mod reference {
    pub use crate::reference_impl::{CommandInfo, markdown, model, snippets};
}
// Standalone HTML rendering, shared by the CLI's export and the browser.
pub mod rendering {
    pub use crate::rendering_impl::{fragment, line_classes};
}
// The editing session a host keeps open per workspace.
pub mod session {
    pub use crate::session_impl::{RefreshReport, Session, SessionRefresh};
}
// The document outline and folding ranges.
pub mod symbols {
    pub use crate::symbols_impl::{flat_symbols, folding_ranges};
}
// On-type formatting triggers.
pub mod typing {
    pub use crate::typing_impl::{TRIGGERS, on_type};
}
