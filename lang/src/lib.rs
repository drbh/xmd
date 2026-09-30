//! The language: its one public interface, in front of the private crates
//! under `lang/core` (`common`, `syntax`, `model`, `values`, `modules`,
//! `eval`). Code outside `lang/` reaches the language only through here, so
//! the core can move items between its crates as long as these paths hold.
//! Each namespace lists, item by item, what the runtime and the hosts use,
//! and nothing else.

/// The shared kernel: spans, file names and URIs.
pub mod common {
    pub use ::common::{
        EXTENSION, LIBRARY_EXTENSION, Span, file_path, file_url, is_library, is_note, library_file,
        note_extension, note_file, note_stem, uri, uri_from_url, url_from_uri,
    };
}

/// The lexer's tokens and literal values, and the one table of task and
/// appointment attributes.
pub mod syntax {
    pub use ::syntax::{
        AttributeKey, AttributeValue, Lexeme, Literal, is_relative_date, literal, stamp,
        valid_expression,
    };
}

/// The contract with the standard library: every function native code calls
/// in a bundled .xmd module, declared once and called through typed functions.
/// `book/reference/contract.md` is generated from it.
pub mod stdlib {
    pub use ::eval::stdlib::{
        CONTRACT, Caller, Contract, Presented, Role, Snapshot, modules, shown,
    };
    pub mod format {
        pub use ::eval::stdlib::format::{age, glyph, series};
    }
    pub mod today {
        pub use ::eval::stdlib::today::page;
    }
    pub mod task {
        pub use ::eval::stdlib::task::{checklist, hover, toggle};
    }
    pub mod resource {
        pub use ::eval::stdlib::resource::{control, hover, label};
    }
    pub mod plan {
        pub use ::eval::stdlib::plan::{hover, seek_summary, write_edits, write_title};
    }
}

/// A parsed note: its definitions, attributes, imports and highlighting.
pub mod model {
    pub use ::model::{
        Attribute, Document, ExprImports, HighlightKind, LineIndex, Named, TaskState, apply_edits,
        byte_at, end_position, expression_regions, identifier, note_path, utf16,
    };
    /// What the recognizers modules declare found in a note.
    pub mod recognized {
        pub use ::model::recognized::{Group, Match, Paint};
    }
}

/// Evaluation: the workspace, the engine, modules and the value kinds they
/// produce. Every name is listed, so a new item in `eval`, `values` or
/// `modules` stays inside `lang/` until it is deliberately exposed here. The
/// namespaces are the spelling the runtime and hosts use; some gather names
/// that live in `values`, `modules`, `syntax`, `model` or `common`, since
/// those crates are private too.
pub mod eval {
    pub use ::eval::{Clock, RequestContext, Symbol, SymbolKind, Workspace, member_symbol};
    pub use ::values::{EvalError, EvalResult, RecordFields, ToValue, record};
    pub mod engine {
        pub use ::common::ValueType;
        pub use ::eval::{Bindings, Engine, HostPresenting};
        pub use ::syntax::{
            Builtin, Expr, Lexeme, Literal, Operator, Parser, Tier, is_builtin_function, lex,
            lex_with_comments, relative_date, sum_scope_at,
        };
        pub use ::values::{HostObject, Value, literal, next_occurrence, value_json};
    }
    pub mod itinerary {
        pub use ::eval::itinerary::{dates, day_record, display_time, label};
        pub use ::model::itinerary::{Day, KEYS, KINDS, Kind, clock, month_name};
    }
    pub mod link_features {
        pub use ::modules::{LinkFeatures, RefreshFormat, RefreshRequest};
    }
    pub mod lookups {
        pub use ::values::{Lookup, LookupKey, Store, day_place, forecast_from};
    }
    pub mod modules {
        pub use ::eval::CompileModules;
        pub use ::modules::{
            Effect, HOOK_RECORDS, HOOKS, Hook, HookContract, HookRecord, Module, ModuleKind,
            ModuleRegistry, STEPS, StepProtocol, is_module_path,
        };
        pub use ::values::{Collection, from_json, json, record};
    }
    pub mod plans {
        pub use ::eval::PlanValue;
        pub use ::model::plans::{goal, regions, seek_body};
    }
    pub mod resources {
        pub use ::eval::{Resource, ResourcePresenting};
        pub use ::modules::{Cache, Metadata};
    }
    pub mod tables {
        pub use ::eval::TableValue;
        pub use ::eval::tables::{
            literal_value, origin, resolve_reference, table, validate_rename,
        };
        pub use ::model::tables::{Domain, aligned, cells, formatting, grids, line_edit, scope_at};
    }
    pub mod timers {
        pub use ::eval::{Timer, TimerAction, edit_timer as edit_in};
    }
}
