//! The language: its one public interface, in front of the private crates
//! in `core/` (`common`, `syntax`, `document`, `values`, `modules`,
//! `eval`). Code outside `core/` reaches the language only through here, so
//! the core can move items between its crates as long as these paths hold.
//! Each namespace lists, item by item, what the services and the hosts use,
//! and nothing else.

/// The shared kernel: spans, file names and URIs.
pub mod common {
    pub use ::common::{
        EXTENSION, LIBRARY_EXTENSION, Span, file_path, file_url, is_library, is_note, library_file,
        lock, note_extension, note_file, note_stem, uri, uri_from_url, url_from_uri,
    };
}

/// The lexer's tokens and literal values, and what a declared attribute's
/// value can hold.
pub mod syntax {
    pub use ::syntax::{
        AttributeValue, Builtin, Expr, Lexeme, Literal, Operator, Parser, Tier,
        is_builtin_function, is_relative_date, lex, lex_with_comments, literal, relative_date,
        stamp, sum_scope_at, valid_expression,
    };
}

/// The contract with the standard library: every function native code calls
/// in a bundled .xmd module, declared once and called through typed functions.
/// `book/reference/writing-modules.md` is generated from it.
pub mod stdlib {
    pub use ::eval::stdlib::{CONTRACT, Presented, Role, modules, shown};
    pub mod format {
        pub use ::eval::stdlib::format::{age, glyph, series};
    }
    pub mod prelude {
        pub use ::eval::stdlib::prelude::lookup_display;
    }
    pub mod today {
        pub use ::eval::stdlib::today::page;
    }
    pub mod task {
        pub use ::eval::stdlib::task::checklist;
    }
    pub mod resource {
        pub use ::eval::stdlib::resource::{control, hover, label};
    }
}

/// A parsed note: its definitions, attributes, imports and highlighting.
pub mod document {
    pub use ::document::{
        Attribute, Document, ExprImports, HighlightKind, LineIndex, Named, TaskState, apply_edits,
        byte_at, expression_regions, identifier, note_path, utf16,
    };
    /// What the recognizers modules declare found in a note.
    pub mod recognized {
        pub use ::document::recognized::{Group, Match, Paint};
    }
}

/// Evaluation: the workspace, the engine, modules and the value kinds they
/// produce. Every name is listed, so a new item in `eval`, `values` or
/// `modules` stays inside `core/` until it is deliberately exposed here. The
/// namespaces are the spelling the services and hosts use; some gather names
/// that live in `values`, `modules`, `syntax`, `document` or `common`, since
/// those crates are private too.
pub mod eval {
    pub use ::eval::{
        Clock, Evaluations, RequestContext, Symbol, SymbolKind, Workspace, member_symbol,
        reads_clock,
    };
    pub use ::values::{EvalError, EvalResult, RecordFields, ToValue, record};
    pub mod engine {
        pub use ::common::ValueType;
        pub use ::eval::{Bindings, Engine, HostPresenting};
        pub use ::values::{HostObject, Measured, Value, claimed, literal, value_json};
    }
    pub mod link_features {
        pub use ::modules::{LinkFeatures, RefreshFormat, RefreshRequest};
    }
    pub mod lookups {
        pub use ::values::{Lookup, LookupKey, Store};
    }
    pub mod modules {
        pub use ::eval::CompileModules;
        pub use ::modules::{
            Declared, Effect, HOOK_RECORDS, HOOKS, Hook, Joins, Module, ModuleKind, ModuleRegistry,
            STEPS, is_module_path, no_clock,
        };
        pub use ::values::{Collection, from_json, json};
    }
    /// The definitions that call a form a module declares: the call, and how
    /// the host reads each expression it is handed.
    pub mod forms {
        pub use ::document::forms::{Reading, call};
    }
    pub mod resources {
        pub use ::eval::{Resource, ResourcePresenting};
        pub use ::modules::{Cache, Metadata};
    }
    pub mod tables {
        pub use ::document::tables::{Domain, cells, grids, scope_at};
        pub use ::eval::TableValue;
        pub use ::eval::tables::{
            literal_value, origin, resolve_reference, table, validate_rename,
        };
    }
}
