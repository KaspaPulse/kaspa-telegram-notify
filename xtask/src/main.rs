mod sbom;
mod security;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "xtask", about = "Kaspa Pulse repository-native Rust tooling")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Security {
        #[command(subcommand)]
        command: SecurityCommand,
    },
    Sbom {
        #[command(subcommand)]
        command: SbomCommand,
    },
}

#[derive(Debug, Subcommand)]
enum SecurityCommand {
    EnvironmentBoundary,
    KaspaPins {
        #[arg(long)]
        expected_version: Option<String>,
        #[arg(long)]
        expected_rev: Option<String>,
    },
    Advisories {
        #[arg(long, default_value_t = 45)]
        max_age_days: i64,
    },
    ScorecardFilter {
        input: PathBuf,
        output: PathBuf,
    },
    ActionPins,
    NonRustExec,
}

#[derive(Debug, Subcommand)]
enum SbomCommand {
    Finalize {
        path: PathBuf,
        #[arg(long)]
        version: String,
        #[arg(long)]
        target: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Security { command } => match command {
            SecurityCommand::EnvironmentBoundary => security::environment_boundary("."),
            SecurityCommand::KaspaPins {
                expected_version,
                expected_rev,
            } => security::kaspa_pins(".", expected_version.as_deref(), expected_rev.as_deref()),
            SecurityCommand::Advisories { max_age_days } => security::advisories(".", max_age_days),
            SecurityCommand::ScorecardFilter { input, output } => {
                security::scorecard_filter(&input, &output)
            }
            SecurityCommand::ActionPins => security::action_pins("."),
            SecurityCommand::NonRustExec => security::non_rust_exec("."),
        },
        Command::Sbom { command } => match command {
            SbomCommand::Finalize {
                path,
                version,
                target,
            } => sbom::finalize(&path, &version, &target),
        },
    }
}
