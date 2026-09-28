//! The runtime every host shares, and the source of truth for runtime
//! behavior: one public interface in front of the private crates beside it
//! (`services`, `host`, `renderer`). Hosts reach the runtime only through here.

/// The language services, the same on every host: requests, sessions,
/// queries, rendering and what an editor shows and does. Every name is listed.
pub mod services {
    pub use ::services::Request;
    pub mod actions {
        pub use ::services::actions::{TaskToggle, apply_edits};
    }
    pub mod agenda {
        pub use ::services::agenda::today_markdown;
    }
    pub mod commands {
        pub use ::services::commands::{Action, Capabilities, PreparedAction};
    }
    pub mod diagnostics {
        pub use ::services::diagnostics::severity_name;
    }
    pub mod hierarchy {
        pub use ::services::hierarchy::{decode, dependencies, dependents, prepare, ranges};
    }
    pub mod intelligence {
        pub use ::services::intelligence::{occurrences, signature, symbol_at};
    }
    pub mod presentation {
        pub use ::services::presentation::{TOKEN_MODIFIERS, TOKEN_TYPES, semantic_tokens};
    }
    pub mod query {
        pub use ::services::query::{Query, display};
    }
    pub mod rendering {
        pub use ::services::rendering::{fragment, line_classes};
    }
    pub mod session {
        pub use ::services::session::{RefreshReport, WorkspaceSession};
    }
    pub mod symbols {
        pub use ::services::symbols::{flat_symbols, folding_ranges};
    }
    pub mod typing {
        pub use ::services::typing::{TRIGGERS, on_type};
    }
}

/// Native input and output: loading workspaces, refreshing, the clock and
/// running command modules. Absent on the web, where there are no files.
#[cfg(not(target_arch = "wasm32"))]
pub mod host {
    pub use ::host::{
        DiskFiles, WorkspaceFiles, fetch_link, load_modules, now, refresh_workspace, run_command,
    };
}
