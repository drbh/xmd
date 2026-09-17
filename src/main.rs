use clap::Parser;
use jot::cli::{Cli, Command};

#[tokio::main]
async fn main() {
    match Cli::parse().command {
        None | Some(Command::Lsp) => jot::editor::serve().await,
        Some(command) => {
            if let Err(error) = jot::cli::run(command).await {
                eprintln!("jot: {error}");
                std::process::exit(1);
            }
        }
    }
}
