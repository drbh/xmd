//! What makes a file a note: its extension, named once. Every check, file
//! name and message the engine, the command line, the language server and the
//! browser build derive from here, so renaming the format means changing
//! the extension macros and renaming the files.
//!
//! There are two extensions and one language. A working note, Markdown with
//! values computed on top, is `.x.md`: GitHub, previews and plain editors show
//! it as Markdown, and only tools that know the suffix add the values. A file
//! of definitions for other files to import (the stdlib, plugins, modules) is
//! `.xmd`, since it is not a document anyone reads as Markdown. Every tool
//! accepts both; which one a file should use is a naming convention, checked
//! by the `file-name` diagnostic, not a difference in how it is read.
//!
//! `.x.md` is a two-part suffix, which `Path::extension` (only `md`) cannot
//! see, so everything here matches whole file-name suffixes.
//!
//! Outside Rust, the extensions are also spelled in the web client's
//! `client/web/src/extension.js` (the docs app and the cloud worker import it),
//! and in declarative editor settings that cannot import anything: the VS Code
//! extension's `package.json` (`languages[].filenamePatterns`), Zed's
//! `languages/xmd/config.toml` (`path_suffixes`), Helix's `languages.toml`
//! (`file-types`) and Neovim's file-type detection.
//!
//! The `.xmd/` workspace directory, the `xmd` binary and the editor language
//! id are separate names and do not follow the extensions.
use std::path::Path;

/// The working-note extension, without its leading dot, as a literal. A
/// macro rather than only a constant so `concat!` can build literals from it,
/// such as static help text.
#[macro_export]
macro_rules! note_extension {
    () => {
        "x.md"
    };
}

/// The library extension, without its leading dot, as a literal: the
/// `include_str!` paths of the bundled stdlib and plugins are built from it.
#[macro_export]
macro_rules! library_extension {
    () => {
        "xmd"
    };
}

/// The extension of a working note, without its leading dot.
pub const EXTENSION: &str = note_extension!();

/// The extension of a file of definitions for others to import, without its
/// leading dot.
pub const LIBRARY_EXTENSION: &str = library_extension!();

/// The file name without either extension, and whether it was the library
/// one, or `None` when the name is neither (including a bare `.x.md` or
/// `.xmd` with no stem).
fn split(name: &str) -> Option<(&str, bool)> {
    let stem = |extension| {
        name.strip_suffix(extension)?
            .strip_suffix('.')
            .filter(|stem: &&str| !stem.is_empty())
    };
    stem(EXTENSION)
        .map(|s| (s, false))
        .or_else(|| stem(LIBRARY_EXTENSION).map(|s| (s, true)))
}

fn file_name(path: &Path) -> Option<&str> {
    path.file_name().and_then(|name| name.to_str())
}

/// Whether a path names a file in the language by its extension: a working
/// note (`.x.md`) or a library (`.xmd`). Everything reads both the same way.
pub fn is_note(path: impl AsRef<Path>) -> bool {
    file_name(path.as_ref()).is_some_and(|name| split(name).is_some())
}

/// Whether a path carries the library extension, `.xmd`.
pub fn is_library(path: impl AsRef<Path>) -> bool {
    file_name(path.as_ref()).is_some_and(|name| split(name).is_some_and(|(_, library)| library))
}

/// A file's name without its extension, for either extension.
pub fn note_stem(path: &Path) -> Option<&str> {
    split(file_name(path)?).map(|(stem, _)| stem)
}

/// A note's file name from its stem: `note_file("trip")` is `trip.x.md`.
pub fn note_file(stem: &str) -> String {
    format!("{stem}.{EXTENSION}")
}

/// A library's file name from its stem: `library_file("units")` is `units.xmd`.
pub fn library_file(stem: &str) -> String {
    format!("{stem}.{LIBRARY_EXTENSION}")
}
