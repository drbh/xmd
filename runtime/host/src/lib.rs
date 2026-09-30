//! The language's input and output on a native host. `lang/core` is pure: it
//! parses, evaluates and presents notes it is handed. This crate does what a
//! desktop host adds: reading notes, modules, the link cache and cached
//! lookups from disk, following imports to files, running the refresh
//! programs link modules request, fetching lookups, parsing feeds, and
//! performing the effects a command module asks for under `xmd run`. Its
//! `DiskFiles` is how an editing session follows imports, so the language services
//! never touch the disk. Exposes its interface from the root.
mod clock;
mod command;
mod feeds;
mod files;
mod io;
mod lookups;
mod refresh;

// Loading a workspace, its modules and caches from disk.
pub use files::{DiskFiles, WorkspaceFiles, save_cache};
// Running a command module and the effects it asks for.
pub use command::{MAX_REQUESTS, MAX_STEPS, run_command_named};
// How long a provider may take for one lookup.
pub use lookups::PROVIDER_STEPS;
// The process clock, which `XMD_NOW` can freeze.
pub use clock::now;
// Refreshing what a workspace reads from outside: link statuses and lookups.
pub use refresh::{fetch_link, refresh_workspace};
