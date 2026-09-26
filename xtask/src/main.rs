mod operations;
mod process;
mod sbom;
mod security;
mod updater;

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
    Database {
        #[command(subcommand)]
        command: DatabaseCommand,
    },
    Ci {
        #[command(subcommand)]
        command: CiCommand,
    },
    Maintenance {
        #[command(subcommand)]
        command: MaintenanceCommand,
    },
    Dependencies {
        #[command(subcommand)]
        command: DependenciesCommand,
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
    SecretScan,
    RustHardening,
    AdminWebhookHardening,
    Pipeline,
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

#[derive(Debug, Subcommand)]
enum DatabaseCommand {
    Backup,
    Migrate,
    Restore { backup_file: PathBuf },
}

#[derive(Debug, Subcommand)]
enum CiCommand {
    PreparePostgres,
}

#[derive(Debug, Subcommand)]
enum MaintenanceCommand {
    CleanHistory {
        #[arg(long)]
        confirm: String,
        #[arg(long, default_value_t = false)]
        push: bool,
    },
}

#[derive(Debug, Subcommand)]
enum DependenciesCommand {
    RustyKaspa {
        #[command(subcommand)]
        command: RustyKaspaCommand,
    },
}

#[derive(Debug, Subcommand)]
enum RustyKaspaCommand {
    Check {
        #[arg(long, default_value_t = false, action = clap::ArgAction::Set)]
        allow_prerelease: bool,
    },
    Update {
        #[arg(long, default_value_t = false, action = clap::ArgAction::Set)]
        allow_prerelease: bool,
        #[arg(long, default_value_t = false)]
        no_branch: bool,
        #[arg(long, default_value = "dev")]
        base_branch: String,
    },
    Publish {
        #[arg(long, default_value = "dev")]
        base_branch: String,
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
            SecurityCommand::SecretScan => operations::secret_scan(),
            SecurityCommand::RustHardening => operations::rust_hardening(),
            SecurityCommand::AdminWebhookHardening => operations::admin_webhook_hardening(),
            SecurityCommand::Pipeline => operations::security_pipeline(),
        },
        Command::Sbom { command } => match command {
            SbomCommand::Finalize {
                path,
                version,
                target,
            } => sbom::finalize(&path, &version, &target),
        },
        Command::Database { command } => match command {
            DatabaseCommand::Backup => operations::db_backup(),
            DatabaseCommand::Migrate => operations::db_migrate(),
            DatabaseCommand::Restore { backup_file } => operations::db_restore(&backup_file),
        },
        Command::Ci { command } => match command {
            CiCommand::PreparePostgres => operations::ci_prepare_postgres(),
        },
        Command::Maintenance { command } => match command {
            MaintenanceCommand::CleanHistory { confirm, push } => {
                operations::clean_history(&confirm, push)
            }
        },
        Command::Dependencies { command } => match command {
            DependenciesCommand::RustyKaspa { command } => match command {
                RustyKaspaCommand::Check { allow_prerelease } => updater::check(allow_prerelease),
                RustyKaspaCommand::Update {
                    allow_prerelease,
                    no_branch,
                    base_branch,
                } => updater::update(allow_prerelease, no_branch, &base_branch),
                RustyKaspaCommand::Publish { base_branch } => updater::publish(&base_branch),
            },
        },
    }
}
