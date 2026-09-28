//! The native hosts: the command line and the language server over stdio,
//! plus the sync helper they share. `xmd`'s `native` feature pulls this in
//! and its `main.rs` is a one-liner calling [`main`]. Exposes its interface
//! from the root.

mod cli;
mod editor;
mod sync;

use clap::{CommandFactory, Parser};
use cli::{Cli, Command};

/// Parses the command line and runs it on a multi-threaded runtime.
#[tokio::main]
pub async fn main() {
    let parsed = Cli::parse();
    // No subcommand means a query; the language server is asked for by name.
    let command = match parsed.command {
        Some(Command::Lsp) => return editor::serve().await,
        Some(command) => command,
        None if parsed.query.input.is_none() && !parsed.query.workspace => {
            let _ = Cli::command().print_help();
            println!();
            return;
        }
        None => Command::Query(parsed.query),
    };
    if let Err(error) = cli::run(command).await {
        eprintln!("xmd: {error}");
        std::process::exit(1);
    }
}
