//! What a note computes: the evaluator and the value kinds it produces
//! (timers, resources, lookups, plans), plus the text charts and the glyph
//! vocabulary every label draws from.
//!
//! `error` is the one failure vocabulary every one of them answers with, so a
//! caller can tell a mistake in a note from data that is merely unfetched
//! without reading the sentence.
//! `modules` compiles the .wtf feature, link and library modules and `context`
//! holds the one workspace snapshot, clock and memo a request evaluates
//! against. `link_features` is the contract a link module implements, next to
//! the module compiler that produces it. `itinerary` resolves the days and
//! stops `model::itinerary` parsed by calling the `itinerary_core` module.
//! Exposes its interface from the root.
// Below, `#[path = "x.rs"] mod x_impl;` keeps a file's module private under an
// `_impl` name; the `pub mod x { pub use crate::x_impl::{...}; }` right after
// it is the actual published namespace, curated to what other crates use by
// `eval::x::` path. Kept wherever a name collides across files (`parse`) or
// forms one coherent family (`engine`), or another crate already spells a
// name that way.
#[path = "charts.rs"]
mod charts_impl;
#[path = "clock.rs"]
mod clock_impl;
mod context;
#[path = "engine/mod.rs"]
mod engine_impl;
mod error;
#[cfg(feature = "native")]
mod feeds;
#[path = "functional.rs"]
mod functional_impl;
#[path = "glyphs.rs"]
mod glyphs_impl;
mod imports;
#[path = "itinerary.rs"]
mod itinerary_impl;
#[path = "link_features.rs"]
mod link_features_impl;
#[path = "lookups.rs"]
mod lookups_impl;
#[path = "modules/mod.rs"]
mod modules_impl;
#[path = "plans.rs"]
mod plans_impl;
mod records;
#[path = "resources.rs"]
mod resources_impl;
mod solver;
#[path = "tables.rs"]
mod tables_impl;
#[path = "timers.rs"]
mod timers_impl;
mod workspace;

// The request context every feature evaluates against, and its clock.
pub use context::{Clock, RequestContext};
// The failure vocabulary every evaluation answers with.
pub use error::{EvalError, EvalResult};
// The note graph: parsed documents, resolved by path and name.
pub use workspace::{Symbol, SymbolKind, Workspace};
// An import's target member, resolved against the note that defines it.
pub use imports::member_symbol;
// A value formatted the way a module's `record` builtin expects.
pub use records::{RecordFields, ToValue};

// The evaluator and the value kinds it produces.
pub mod engine {
    pub use crate::engine_impl::{
        Bindings, Builtin, Engine, Expr, HostObject, Lexeme, Literal, Operator, Parser, Tier,
        Value, ValueType, is_builtin_function, lex, lex_with_comments, literal, next_occurrence,
        relative_date, sum_scope_at, value_json,
    };
}
// The one clock a note's dates and durations are read against.
pub mod clock {
    pub use crate::clock_impl::now;
}
// Text charts a hover renders next to a plan or a timer.
pub mod charts {
    pub use crate::charts_impl::{bar, bar_fraction, series};
}
// The glyph every label in a hover, action or diagnostic draws from.
pub mod glyphs {
    pub use crate::glyphs_impl::{DONE, FLAG, OFF, OPEN, REFRESH, REPEAT};
}
// Pure functions the evaluator's builtins are implemented in terms of.
pub mod functional {
    pub use crate::functional_impl::{compare, sum};
}
// Days and stops resolved from `model::itinerary`; each of tables, plans and
// itinerary defines its own `parse`, so these stay namespaced.
pub mod itinerary {
    pub use crate::itinerary_impl::{
        Day, KEYS, KINDS, Kind, clock, dates, day_record, display_time, label, month_name,
        try_dates,
    };
}
pub mod plans {
    pub use crate::plans_impl::{goal, regions, seek_body};
}
pub mod tables {
    pub use crate::tables_impl::{
        Domain, aligned, cells, formatting, grids, line_edit, origin, resolve_reference, scope_at,
        table, validate_rename,
    };
}
// The contract a link module implements, and the built-in ones.
pub mod link_features {
    pub use crate::link_features_impl::{BUILTINS, LinkFeatures};
}
// Fetched lookup values, and (native only) how to refresh them.
pub mod lookups {
    #[cfg(feature = "native")]
    pub use crate::lookups_impl::native;
    pub use crate::lookups_impl::{LookupKey, day_place, forecast_from};
}
// The .wtf feature, link and library modules the engine calls into.
pub mod modules {
    pub use crate::modules_impl::{
        Collection, Hook, Module, ModuleKind, ModuleRegistry, from_json, is_module_path, json,
        record,
    };
}
// A resource value and how a hover presents it.
pub mod resources {
    pub use crate::resources_impl::{Resource, ResourcePresenting, ago};
}
// Timer edits a command line or editor action can apply.
pub mod timers {
    pub use crate::timers_impl::{TimerAction, edit_in};
}
