//! XMD, Markdown with computed values: a facade in front of its layered crates.
//! This crate has no code of its own — it only re-exports the published
//! interface of each layer's host-facing pieces:
//!
//! - `common` is the shared kernel (`Span`, `Resource`, value kinds); every
//!   layer below depends on it, none of them on each other out of order.
//! - `syntax` -> `model` -> `eval` -> `services` is the pipeline: grammar,
//!   then parsing, then evaluation, then editor behavior. Each stage only
//!   calls downwards.
//! - `runtime` holds what every host shares: `services` (language
//!   services) and `host` (native I/O). `lsp` (the language server), this
//!   crate's `xmd` binary (the command line) and `browser` (the wasm host) are
//!   built on it; the binary is the only crate that assembles hosts.
//!
//! What this crate exposes is only what a host binary or the test harness
//! needs by name: [`byte_at`] and [`actions`] from the pipeline, [`is_note`]
//! from the kernel, and the browser host as `browser`.
#[cfg(feature = "browser")]
pub use browser;
pub use common::is_note;
pub use model::byte_at;
pub use services::actions;
