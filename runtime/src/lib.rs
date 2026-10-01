//! The runtime every host shares, and the source of truth for runtime
//! behavior: one public interface in front of the private crates beside it
//! (`analysis`, `catalog`, `editor`, `host`). Hosts reach the runtime only
//! through here, and every name they can reach is listed, so nothing is
//! exposed by accident.

/// The language services, the same on every host: requests, sessions,
/// queries, rendering and what an editor shows and does. One namespace over
/// three crates: language facts (`analysis`), records and queries (`catalog`)
/// and the editing features and session (`editor`).
pub mod services {
    pub use ::analysis::{
        BUILTINS, Outcome, Signature, definition, flat_symbols, folding_ranges, highlights,
        references, rename, severity_name, signature, symbol_at,
    };
    pub use ::catalog::{Query, display};
    pub use ::editor::{
        RefreshReport, Request, RowActions, TOKEN_MODIFIERS, TOKEN_TYPES, WorkspaceSession,
        fragment, line_classes, semantic_tokens, today_markdown,
    };
    pub mod commands {
        pub use ::editor::{Action, Capabilities, PreparedAction};
    }
    pub mod hierarchy {
        pub use ::analysis::hierarchy::{decode, dependencies, dependents, prepare, ranges};
    }
    pub mod typing {
        pub use ::editor::TRIGGERS;
    }
}

/// Native input and output: loading workspaces, refreshing, the clock and
/// running command modules. Absent on the web, where there are no files.
#[cfg(not(target_arch = "wasm32"))]
pub mod host {
    pub use ::host::{
        DiskFiles, MAX_REQUESTS, MAX_STEPS, PROVIDER_STEPS, WorkspaceFiles, fetch_link, now,
        refresh_workspace, run_command_named, save_cache,
    };
}
