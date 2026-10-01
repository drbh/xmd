//! Language facts about a workspace, the same on every host: what sits at a
//! position, how a symbol is named and described, its hover, signature,
//! outline, occurrences and call hierarchy, the native diagnostics and the
//! refactors offered over a range. Everything here reads the workspace and
//! returns LSP-shaped values; nothing edits, renders or runs modules.
//! Exposes its interface from the root, as one list of names.
//!
//! `locate` decides what sits at a position, and `describe` names a symbol
//! the same way for the outline, the call hierarchy and completion.
// `describe` and `hierarchy` load as private `_impl` modules so their names
// are free for the curated namespaces at the bottom.
#[path = "describe.rs"]
mod describe_impl;
mod diagnostics;
#[path = "hierarchy.rs"]
mod hierarchy_impl;
mod hover;
mod locate;
mod navigation;
mod refactor;
mod signature;
mod symbols;

pub use diagnostics::{collect_native, module_problem, severity_name};
pub use hover::{RowHover, SymbolHover, hover_at, markup, source_link, symbol_hover};
pub use locate::{inert, symbol_at};
pub use navigation::{definition, highlights, references, rename};
pub use refactor::{CodeActionItem, refactors};
pub use signature::{BUILTINS, Outcome, Signature, call_context, signature};
pub use symbols::{Outlined, document_symbols, flat_symbols, folding_ranges, line_range};

// Names too generic to stand alone keep a namespace.
pub mod describe {
    pub use crate::describe_impl::{detail, summary};
}
pub mod hierarchy {
    pub use crate::hierarchy_impl::{
        decode, dependencies, dependents, encode, item, label, nodes, prepare, ranges, selection,
    };
}
