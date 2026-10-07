use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use codex_team::self_improvement::{run_supervisor, LearningVault};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "codex-team-supervisor", version)]
#[command(about = "Stable safety and A/B recovery boundary for codex-team self-improvement")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the user-scoped background supervisor.
    Run {
        /// Exact enrolled vault root. Defaults to ~/.codex-team/self-improvement.
        #[arg(long)]
        vault: Option<PathBuf>,
        /// Perform one health/recovery tick and exit.
        #[arg(long)]
        once: bool,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Run { vault, once } => {
            let vault = match vault {
                Some(path) => LearningVault::existing_at(path)?
                    .context("self-improvement vault does not exist")?,
                None => LearningVault::existing_machine()?
                    .context("self-improvement is not enrolled")?,
            };
            run_supervisor(vault, once)
        }
    }
}
