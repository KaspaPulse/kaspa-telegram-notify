mod operations;
mod opqual;
mod process;
mod proof;
mod sbom;
mod security;
mod updater;

use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use std::{fs, path::PathBuf};

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
    Proof {
        #[command(subcommand)]
        command: ProofCommand,
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
    Opqual {
        #[command(subcommand)]
        command: OpqualCommand,
    },
    PackageVersion,
}

#[derive(Debug, Subcommand)]
enum SecurityCommand {
    EnvironmentBoundary,
    Documentation,
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
enum ProofCommand {
    RustOnly,
    Native,
    Supply,
    Verify,
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
enum OpqualCommand {
    DryRun {
        #[arg(long, default_value = "target/release/kaspa-pulse")]
        binary: PathBuf,
    },
    Run {
        #[arg(long, default_value = "target/release/kaspa-pulse")]
        binary: PathBuf,
    },
    Resume {
        run_id: String,
        #[arg(long, default_value = "target/release/kaspa-pulse")]
        binary: PathBuf,
    },
    Impact {
        #[arg(long)]
        base_sha: String,
        #[arg(long)]
        tested_sha: String,
        #[arg(long)]
        baseline_proof: Option<PathBuf>,
        #[arg(long, default_value = "target/opqual-impact-plan.json")]
        output: PathBuf,
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
        #[arg(long, default_value = "main")]
        base_branch: String,
    },
    Publish {
        #[arg(long, default_value = "main")]
        base_branch: String,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Security { command } => match command {
            SecurityCommand::EnvironmentBoundary => security::environment_boundary("."),
            SecurityCommand::Documentation => security::documentation("."),
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
        Command::Proof { command } => match command {
            ProofCommand::RustOnly => proof::emit_rust_only("."),
            ProofCommand::Native => proof::emit_native("."),
            ProofCommand::Supply => proof::emit_supply("."),
            ProofCommand::Verify => proof::verify_all("."),
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
        Command::Opqual { command } => match command {
            OpqualCommand::DryRun { binary } => {
                opqual::execute(opqual::Mode::DryRun, &binary, None)
            }
            OpqualCommand::Run { binary } => opqual::execute(opqual::Mode::Run, &binary, None),
            OpqualCommand::Resume { run_id, binary } => {
                opqual::execute(opqual::Mode::Resume, &binary, Some(&run_id))
            }
            OpqualCommand::Impact {
                base_sha,
                tested_sha,
                baseline_proof,
                output,
            } => opqual::impact(&base_sha, &tested_sha, baseline_proof.as_deref(), &output),
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
        Command::PackageVersion => package_version(),
    }
}

fn package_version() -> Result<()> {
    let cargo: toml::Value = fs::read_to_string("Cargo.toml")?
        .parse()
        .context("failed to parse Cargo.toml")?;
    let version = cargo
        .get("package")
        .and_then(toml::Value::as_table)
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        .context("Cargo.toml package.version missing")?;
    let valid = regex::Regex::new(r"^[0-9]+[.][0-9]+[.][0-9]+(?:[-+][0-9A-Za-z.-]+)?$")?;
    ensure!(
        valid.is_match(version),
        "invalid package version: {version}"
    );
    println!("{version}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rusty_kaspa_update_defaults_to_main() {
        let cli = Cli::try_parse_from(["xtask", "dependencies", "rusty-kaspa", "update"])
            .expect("parse update command");
        let Command::Dependencies {
            command:
                DependenciesCommand::RustyKaspa {
                    command: RustyKaspaCommand::Update { base_branch, .. },
                },
        } = cli.command
        else {
            panic!("unexpected command");
        };
        assert_eq!(base_branch, "main");
    }

    #[test]
    fn rusty_kaspa_publish_defaults_to_main() {
        let cli = Cli::try_parse_from(["xtask", "dependencies", "rusty-kaspa", "publish"])
            .expect("parse publish command");
        let Command::Dependencies {
            command:
                DependenciesCommand::RustyKaspa {
                    command: RustyKaspaCommand::Publish { base_branch },
                },
        } = cli.command
        else {
            panic!("unexpected command");
        };
        assert_eq!(base_branch, "main");
    }
}
