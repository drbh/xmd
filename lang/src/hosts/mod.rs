//! How the features are delivered: the native language server over stdio,
//! the WebAssembly workspace for the browser, and the command line.
#[cfg(feature = "browser")]
pub mod browser;
#[cfg(feature = "native")]
pub mod cli;
#[cfg(feature = "native")]
pub mod editor;
