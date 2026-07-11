use std::path::PathBuf;

use wormholesystems_cli::{docker, wizard};

use anyhow::Result;
use clap::{Parser, Subcommand};

/// Interactive setup wizard for the Wormhole Systems container stack.
#[derive(Parser)]
#[command(name = "wsctl", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the interactive setup wizard (default)
    Setup {
        /// Path to an existing wormholesystems-containers checkout.
        /// If omitted, the current directory is used when it looks like
        /// the repo; otherwise the wizard offers to clone it.
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Check that git, docker and docker compose are available
    Doctor,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Setup { dir: None }) {
        Command::Setup { dir } => wizard::run(dir),
        Command::Doctor => {
            if !docker::doctor()? {
                anyhow::bail!("the docker daemon is not running — start Docker and try again");
            }
            Ok(())
        }
    }
}
