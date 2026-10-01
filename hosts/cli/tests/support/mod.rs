//! Shared plumbing for the end-to-end suites: the snapshot cases
//! (`tests/snapshots.rs`), the book and the reference pages.
#![allow(dead_code)]

pub(crate) mod lsp;

use std::{path::Path, process::Command};

/// The clock every suite freezes, unless a snapshot case sets its own.
pub(crate) const NOW: &str = "2026-09-16T14:00:00-04:00";

/// `UPDATE_SNAPSHOTS=1` rewrites the expected output instead of comparing.
pub(crate) fn update_snapshots() -> bool {
    std::env::var("UPDATE_SNAPSHOTS").is_ok_and(|v| !v.is_empty() && v != "0")
}

/// Freezes a run's clock at `now` (`XMD_NOW`) and takes personal modules from
/// `config` (`XDG_CONFIG_HOME`), never the developer's.
pub(crate) fn isolate<'a>(command: &'a mut Command, config: &Path, now: &str) -> &'a mut Command {
    command.env("XDG_CONFIG_HOME", config).env("XMD_NOW", now)
}
