//! The runtime every host shares, and the source of truth for runtime
//! behavior: one public interface in front of the private crates beside it
//! (`services`, `host`, `renderer`). Hosts reach the runtime only through here.

/// The language services, the same on every host: requests, sessions,
/// queries, rendering and what an editor shows and does.
pub mod services {
    pub use ::services::{
        Request, actions, agenda, commands, diagnostics, hierarchy, intelligence, presentation,
        query, rendering, session, symbols, typing,
    };
}

/// Native input and output: loading workspaces, refreshing, the clock and
/// running command modules. Absent on the web, where there are no files.
#[cfg(not(target_arch = "wasm32"))]
pub mod host {
    pub use ::host::{
        DiskFiles, WorkspaceFiles, fetch_link, load_modules, now, refresh_workspace, run_command,
    };
}
