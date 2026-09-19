use clap::Parser;
use wtf::cli::{Cli, Command};

#[tokio::main]
async fn main() {
    match Cli::parse().command {
        None | Some(Command::Lsp) => wtf::editor::serve().await,
        Some(command) => {
            if let Err(error) = wtf::cli::run(command).await {
                eprintln!("wtf: {error}");
                std::process::exit(1);
            }
        }
    }
}
