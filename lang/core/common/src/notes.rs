//! What makes a file a note: its extension, named once. Every check, file
//! name and message the engine, the command line, the language server and the
//! browser build derive from here, so renaming the format means changing
//! `note_extension!` and renaming the files.
//!
//! Notes are Markdown with values computed on top, so the extension ends in
//! `.md`: GitHub, previews and plain editors show a note as Markdown, and only
//! tools that know the `.x.md` suffix add the computed values. That makes the
//! extension a two-part suffix, which `Path::extension` (only `md`) cannot
//! see, so everything here matches whole file-name suffixes.
//!
//! Outside Rust, the extension is also spelled in the web client's
//! `client/web/src/extension.js` (the docs app and the cloud worker import it),
//! and in declarative editor settings that cannot import anything: the VS Code
//! extension's `package.json` (`languages[].filenamePatterns`), Zed's
//! `languages/xmd/config.toml` (`path_suffixes`), Helix's `languages.toml`
//! (`file-types`) and Neovim's file-type detection.
//!
//! The `.xmd/` workspace directory, the `xmd` binary and the editor language
//! id are separate names and do not follow the extension.
use std::path::Path;

/// The note extension, without its leading dot, as a literal. A macro rather
/// than only a constant so `concat!` can build literals from it, such as the
/// `include_str!` paths of the bundled stdlib and static help text.
#[macro_export]
macro_rules! note_extension {
    () => {
        "x.md"
    };
}

/// The extension every note and module file carries, without its leading dot.
pub const EXTENSION: &str = note_extension!();

/// A note file name without its extension, or `None` when the name is not a
/// note's (including a bare `.x.md` with no stem).
fn note_stem(name: &str) -> Option<&str> {
    name.strip_suffix(EXTENSION)?
        .strip_suffix('.')
        .filter(|stem| !stem.is_empty())
}

/// Whether a path names a note or module file by its extension.
pub fn is_note(path: impl AsRef<Path>) -> bool {
    path.as_ref()
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| note_stem(name).is_some())
}

/// A note's file name from its stem: `note_file("trip")` is `trip.x.md`.
pub fn note_file(stem: &str) -> String {
    format!("{stem}.{EXTENSION}")
}
