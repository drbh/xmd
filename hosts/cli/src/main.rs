//! The `xmd` command line: queries, rendering, inspection, refresh and
//! command modules, plus `xmd lsp`, which hands the process to the language
//! server. Shared behavior comes from `native` (I/O) and `services`; this
//! binary only parses arguments and reports.
mod cli;

use clap::{CommandFactory, Parser};
use cli::{Cli, Command};

/// Parses the command line and runs it on a multi-threaded runtime.
#[tokio::main]
async fn main() {
    let parsed = Cli::parse();
    // No subcommand means a query; the language server is asked for by name.
    let command = match parsed.command {
        Some(Command::Lsp) => return lsp::serve().await,
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
