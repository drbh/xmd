//! The language: its one public interface, in front of the private crates
//! under `lang/core` (`common`, `syntax`, `model`, `eval`). Code outside
//! `lang/` reaches the language only through here, so the core can move
//! items between its crates as long as these paths hold. Each namespace
//! lists, item by item, what the runtime and the hosts use, and nothing else.

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
/// produce. Every name is listed, so a new item in `eval` stays inside
/// `lang/` until it is deliberately exposed here.
pub mod eval {
    pub use ::eval::{
        Clock, EvalError, EvalResult, RecordFields, RequestContext, Symbol, SymbolKind, ToValue,
        Workspace, member_symbol, record,
    };
    pub mod engine {
        pub use ::eval::engine::{
            Bindings, Builtin, Engine, Expr, HostObject, Lexeme, Literal, Operator, Parser, Tier,
            Value, ValueType, is_builtin_function, lex, lex_with_comments, literal,
            next_occurrence, relative_date, sum_scope_at, value_json,
        };
    }
    pub mod functional {
        pub use ::eval::functional::{compare, sum};
    }
    pub mod itinerary {
        pub use ::eval::itinerary::{
            Day, KEYS, KINDS, Kind, clock, dates, day_record, display_time, label, month_name,
            try_dates,
        };
    }
    pub mod link_features {
        pub use ::eval::link_features::{BUILTINS, LinkFeatures, RefreshFormat};
    }
    pub mod lookups {
        pub use ::eval::lookups::{Lookup, LookupKey, day_place, forecast_from};
    }
    pub mod modules {
        pub use ::eval::modules::{
            Collection, Hook, Module, ModuleKind, ModuleRegistry, from_json, is_module_path, json,
            record,
        };
    }
    pub mod plans {
        pub use ::eval::plans::{goal, regions, seek_body};
    }
    pub mod resources {
        pub use ::eval::resources::{Cache, Metadata, Resource, ResourcePresenting};
    }
    pub mod tables {
        pub use ::eval::tables::{
            Domain, aligned, cells, formatting, grids, line_edit, origin, resolve_reference,
            scope_at, table, validate_rename,
        };
    }
    pub mod timers {
        pub use ::eval::timers::{TimerAction, edit_in};
    }
}
