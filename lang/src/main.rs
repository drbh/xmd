use clap::{CommandFactory, Parser};
use wtf::cli::{Cli, Command};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    // No subcommand means a query; the language server is asked for by name.
    let command = match cli.command {
        Some(Command::Lsp) => return wtf::editor::serve().await,
        Some(command) => command,
        None if cli.query.input.is_none() && !cli.query.workspace => {
            let _ = Cli::command().print_help();
            println!();
            return;
        }
        None => Command::Query(cli.query),
    };
    if let Err(error) = wtf::cli::run(command).await {
        eprintln!("wtf: {error}");
        std::process::exit(1);
    }
}
