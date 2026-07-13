use std::path::PathBuf;

use clap::{Parser, Subcommand};
use console::style;
use wormholesystems_cli::{docker, wizard};

/// Interactive setup wizard for the Wormhole Systems container stack.
#[derive(Parser)]
#[command(name = "wsctl", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the interactive setup wizard
    #[command(visible_alias = "init")]
    Setup {
        /// Path to an existing wormholesystems-containers checkout.
        /// If omitted, the current directory is used when it looks like
        /// the repo; otherwise the wizard offers to clone it.
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Update a running instance's EVE static data (SDE) and migrations
    Update {
        /// Path to the wormholesystems-containers checkout (default: cwd)
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Show this machine's public IP, the DNS records the app needs and
    /// where the configured domains currently point
    Dns {
        /// Path to the wormholesystems-containers checkout (default: cwd)
        #[arg(long)]
        dir: Option<PathBuf>,
    },
    /// Check that git, docker and docker compose are available
    Doctor,
    /// Show version and project links
    About,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        None => {
            print_info();
            Ok(())
        }
        Some(Command::Setup { dir }) => wizard::run(dir),
        Some(Command::Update { dir }) => wizard::update(dir),
        Some(Command::Dns { dir }) => wizard::dns(dir),
        Some(Command::Doctor) => {
            if !docker::doctor()? {
                anyhow::bail!("the docker daemon is not running — start Docker and try again");
            }
            Ok(())
        }
        Some(Command::About) => {
            print_about();
            Ok(())
        }
    }
}

fn print_info() {
    println!(
        "{} {} — set up and manage a Wormhole Systems instance\n",
        style("wsctl").bold(),
        env!("CARGO_PKG_VERSION")
    );
    println!("{}", style("Commands:").bold());
    for (name, description) in [
        ("setup", "run the interactive setup wizard (alias: init)"),
        ("update", "refresh EVE static data of a running instance"),
        ("dns", "show the public IP and required DNS records"),
        ("doctor", "check git, docker and docker compose"),
        ("about", "show version and project links"),
    ] {
        println!("  {:<8} {description}", style(name).cyan());
    }
    println!(
        "\nRun `{}` to get started, `{}` for details.",
        style("wsctl setup").cyan(),
        style("wsctl <command> --help").cyan()
    );
}

fn print_about() {
    println!(
        "{} {}\nInteractive setup wizard for the Wormhole Systems container stack.\n",
        style("wsctl").bold(),
        env!("CARGO_PKG_VERSION")
    );
    for (label, url) in [
        (
            "CLI repo",
            "https://github.com/WormholeSystems/wormholesystems-cli",
        ),
        (
            "Stack repo",
            "https://github.com/WormholeSystems/wormholesystems-containers",
        ),
        ("Installer", "https://install.wormhole.systems"),
        ("App", "https://wormhole.systems"),
    ] {
        println!("  {:<11} {}", label, style(url).cyan());
    }
}
