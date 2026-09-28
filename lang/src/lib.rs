//! The language: its one public interface, in front of the private crates
//! under `lang/core` (`common`, `syntax`, `model`, `eval`). Code outside
//! `lang/` reaches the language only through here, so the core can move
//! items between its crates as long as these paths hold. Each namespace
//! lists what the runtime and the hosts use, and nothing else.

/// The shared kernel: spans, file names and URIs.
pub mod common {
    pub use ::common::{
        EXTENSION, Span, file_path, file_url, is_note, note_extension, note_file, uri,
        uri_from_url, url_from_uri,
    };
}

/// The lexer's tokens and literal values.
pub mod syntax {
    pub use ::syntax::{Lexeme, Literal, literal};
}

/// A parsed note: its definitions, attributes, imports and highlighting.
pub mod model {
    pub use ::model::{
        Attribute, Document, ExprImports, HighlightKind, Named, byte_at, expression_regions,
        identifier, note_path, utf16,
    };
}

/// Evaluation: the workspace, the engine, modules and the value kinds they
/// produce. The namespaces are `eval`'s own curated ones.
pub mod eval {
    pub use ::eval::{
        Clock, EvalError, EvalResult, RecordFields, RequestContext, Symbol, SymbolKind, ToValue,
        Workspace, engine, functional, itinerary, link_features, lookups, member_symbol, modules,
        plans, record, resources, tables, timers,
    };
}
