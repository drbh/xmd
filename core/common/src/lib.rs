//! The shared kernel: types every layer agrees on, so `syntax`, `document`,
//! `eval` and `services` name the same `Span`, `Resource` and value
//! vocabulary instead of each defining their own. No lexer, no parser, no
//! `Literal` (that's the lexer's output, and stays in `syntax`); `common`
//! depends on no other crate in this workspace. Exposes its interface from
//! the root.
mod notes;
mod paths;
mod pattern;
mod resource;
mod span;
mod values;

pub use notes::{
    EXTENSION, LIBRARY_EXTENSION, is_library, is_note, library_file, note_file, note_stem,
};
pub use paths::{file_path, file_url, uri, uri_from_url, url_from_uri};
pub use pattern::{Found, Pattern};
pub use resource::Resource;
pub use span::{LineIndex, Lines, Span};
pub use values::{Code, Currency, ValueType, is_code};
