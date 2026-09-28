//! The language's input and output on a native host. `lang/core` is pure: it
//! parses, evaluates and presents notes it is handed. This crate does what a
//! desktop host adds: reading notes, modules, the link cache and cached
//! lookups from disk, following imports to files, running the refresh
//! programs link modules request, fetching lookups, and parsing feeds.
//! Exposes its interface from the root.
mod feeds;
mod files;
mod lookups_impl;
mod refresh;

// Loading a workspace, its modules and caches from disk.
pub use files::WorkspaceFiles;
// Running a link module's refresh request.
pub use refresh::fetch_link;
// Fetching and saving cached lookups.
pub mod lookups {
    pub use crate::lookups_impl::{load, refresh, save};
}
