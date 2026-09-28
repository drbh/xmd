//! The language's input and output on a native host. `lang/core` is pure: it
//! parses, evaluates and presents notes it is handed. This crate does what a
//! desktop host adds: reading notes, modules, the link cache and cached
//! lookups from disk, following imports to files, running the refresh
//! programs link modules request, fetching lookups, parsing feeds, and
//! performing the effects a command module asks for under `xmd run`.
//! Exposes its interface from the root.
mod clock;
mod command;
mod feeds;
mod files;
mod lookups_impl;
mod refresh;

// Loading a workspace, its modules and caches from disk.
pub use files::{DiskFiles, WorkspaceFiles, load_modules};
// Running a command module and the effects it asks for.
pub use command::run_command;
// The process clock, which `XMD_NOW` can freeze.
pub use clock::now;
// Refreshing what a workspace reads from outside: link statuses and lookups.
pub use refresh::{fetch_link, refresh_workspace};
// Fetching and saving cached lookups.
pub mod lookups {
    pub use crate::lookups_impl::{load, refresh, save};
}
