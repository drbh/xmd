//! What makes a file a note: its extension, named once. Every check, file
//! name and message the engine, the command line, the language server and the
//! browser build derive from here, so renaming the format means changing
//! `note_extension!` and renaming the files.
//!
//! Outside Rust, the extension is also spelled in the web client's
//! `client/web/src/extension.js` (the docs app and the cloud worker import it),
//! and in declarative editor settings that cannot import anything: the VS Code
//! extension's `package.json` (`languages[].extensions`), Zed's
//! `languages/wtf/config.toml` (`path_suffixes`), Helix's `languages.toml`
//! (`file-types`) and Neovim's file-type detection.
//!
//! The `.wtf/` workspace directory, the `wtf` binary and the editor language
//! id are separate names and do not follow the extension.
use std::path::Path;

/// The note extension, without its dot, as a literal. A macro rather than
/// only a constant so `concat!` can build literals from it, such as the
/// `include_str!` paths of the bundled stdlib and static help text.
#[macro_export]
macro_rules! note_extension {
    () => {
        "wtf"
    };
}

/// The extension every note and module file carries, without its dot.
pub const EXTENSION: &str = note_extension!();

/// Whether a path names a note or module file by its extension.
pub fn is_note(path: impl AsRef<Path>) -> bool {
    path.as_ref().extension().is_some_and(|e| e == EXTENSION)
}

/// A note's file name from its stem: `note_file("trip")` is `trip.wtf`.
pub fn note_file(stem: &str) -> String {
    format!("{stem}.{EXTENSION}")
}

/// A note file name without its extension, or `None` when it has another.
pub fn note_stem(name: &str) -> Option<&str> {
    name.strip_suffix(EXTENSION)?.strip_suffix('.')
}
