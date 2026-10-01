//! XMD, Markdown with computed values: a facade in front of its layered crates.
//! This crate has no code of its own — it only re-exports the published
//! interface of each layer's host-facing pieces:
//!
//! - `common` is the shared kernel (`Span`, `Resource`, value kinds); every
//!   layer below depends on it, none of them on each other out of order.
//! - `syntax` -> `document` -> `eval` -> `analysis` -> `records` -> `features`
//!   is the pipeline: grammar, parsing, evaluation, then language facts,
//!   records and editor behavior. Each stage only calls downwards.
//! - `services` holds what every host shares: those three service crates
//!   behind one facade. `native` is native I/O, for the native hosts only.
//!   `lsp` (the language server), this crate's `xmd` binary (the command line)
//!   and `wasm` (the browser host) are built on them; the binary is the only
//!   crate that assembles hosts.
//!
//! What this crate exposes is only what a host binary or the test harness
//! needs by name: [`byte_at`] and [`apply_edits`] from the pipeline, [`is_note`]
//! from the kernel, and the browser host as `wasm`.
pub use lang::common::is_note;
pub use lang::document::{apply_edits, byte_at};
#[cfg(feature = "browser")]
pub use wasm;
