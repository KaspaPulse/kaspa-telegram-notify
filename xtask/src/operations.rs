use crate::process;
use anyhow::{Context, Result, bail, ensure};
use chrono::Utc;
use regex::Regex;
use std::collections::BTreeSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

fn env_required(name: &str) -> Result<String> {
    let value = env::var(name).with_context(|| format!("{name} is required"))?;
    ensure!(!value.trim().is_empty(), "{name} is required");
    Ok(value)
}

fn read_dotenv(name: &str) -> Result<String> {
    let text = fs::read_to_string(".env").context(".env not found")?;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if let Some(value) = line.strip_prefix(&format!("{name}=")) {
            return Ok(value.trim().trim_matches(['\'', '"']).to_owned());
        }
    }
    bail!("{name} not found in .env")
}

fn walk_files(root: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(root).with_context(|| format!("read {}", root.display()))? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if matches!(name.as_ref(), ".git" | "target" | ".sqlx") {
                continue;
            }
            walk_files(&path, out)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

pub fn db_backup() -> Result<()> {
    process::require("pg_dump")?;
    let database_url = read_dotenv("DATABASE_URL")?;
    let dir = Path::new("backups/db");
    fs::create_dir_all(dir)?;
    let name = format!("kaspa-pulse-{}.dump", Utc::now().format("%Y%m%d-%H%M%S"));
    let out = dir.join(name);
    let out_arg = format!("--file={}", out.display());
    process::run("pg_dump", [&database_url, "--format=custom", &out_arg])?;
    println!("database-backup: PASS path={}", out.display());
    Ok(())
}

pub fn db_restore(backup: &Path) -> Result<()> {
    process::require("pg_restore")?;
    ensure!(
        backup.is_file(),
        "backup file not found: {}",
        backup.display()
    );
    let database_url = read_dotenv("DATABASE_URL")?;
    let db_arg = format!("--dbname={database_url}");
    let backup_arg = backup.display().to_string();
    process::run(
        "pg_restore",
        ["--clean", "--if-exists", "--no-owner", &db_arg, &backup_arg],
    )?;
    println!("database-restore: PASS path={}", backup.display());
    Ok(())
}

pub fn db_migrate() -> Result<()> {
    process::require("psql")?;
    let database_url = read_dotenv("DATABASE_URL")?;
    apply_migrations(&database_url)?;
    println!("database-migrate: PASS");
    Ok(())
}

fn migration_files() -> Result<Vec<PathBuf>> {
    let mut files = fs::read_dir("migrations")
        .context("migrations folder not found")?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|x| x.to_str()) == Some("sql"))
        .collect::<Vec<_>>();
    files.sort();
    ensure!(!files.is_empty(), "no SQL migrations found");
    Ok(files)
}

fn apply_migrations(database_url: &str) -> Result<()> {
    for migration in migration_files()? {
        println!("applying migration: {}", migration.display());
        let path = migration.display().to_string();
        process::run("psql", [database_url, "-v", "ON_ERROR_STOP=1", "-f", &path])?;
    }
    Ok(())
}

pub fn ci_prepare_postgres() -> Result<()> {
    process::require("pg_isready")?;
    process::require("psql")?;
    let admin = env_required("DATABASE_ADMIN_URL")?;
    let runtime = env_required("DATABASE_URL")?;
    let mut ready = false;
    for _ in 0..30 {
        let (ok, _) = process::capture_status("pg_isready", ["--dbname", &admin, "--quiet"])?;
        if ok {
            ready = true;
            break;
        }
        thread::sleep(Duration::from_secs(1));
    }
    ensure!(ready, "PostgreSQL did not become ready");

    let role_sql = r#"DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = 'kaspa_pulse_app') THEN
        CREATE ROLE kaspa_pulse_app LOGIN PASSWORD 'TEST_PASSWORD';
    ELSE
        ALTER ROLE kaspa_pulse_app WITH LOGIN PASSWORD 'TEST_PASSWORD';
    END IF;
END
$$;"#;
    process::run("psql", [&admin, "-v", "ON_ERROR_STOP=1", "-c", role_sql])?;
    apply_migrations(&admin)?;
    let grant_sql = "GRANT TRUNCATE ON TABLE telegram_delivery_queue, wallet_alert_dedup, user_wallets TO kaspa_pulse_app;";
    process::run("psql", [&admin, "-v", "ON_ERROR_STOP=1", "-c", grant_sql])?;

    let verify_sql = r#"
SELECT
    to_regclass('public.telegram_delivery_queue') IS NOT NULL
    AND to_regclass('public.wallet_alert_dedup') IS NOT NULL
    AND to_regclass('public.user_wallets') IS NOT NULL
    AND to_regclass('public.wallet_seen_utxos') IS NOT NULL
    AND to_regclass('public.system_settings') IS NOT NULL
    AND to_regclass('public.telegram_delivery_queue_id_seq') IS NOT NULL
    AND has_table_privilege(current_user,'public.telegram_delivery_queue','SELECT')
    AND has_table_privilege(current_user,'public.telegram_delivery_queue','INSERT')
    AND has_table_privilege(current_user,'public.telegram_delivery_queue','UPDATE')
    AND has_table_privilege(current_user,'public.telegram_delivery_queue','DELETE')
    AND has_table_privilege(current_user,'public.telegram_delivery_queue','TRUNCATE')
    AND has_sequence_privilege(current_user,'public.telegram_delivery_queue_id_seq','USAGE')
    AND has_table_privilege(current_user,'public.wallet_alert_dedup','SELECT')
    AND has_table_privilege(current_user,'public.wallet_alert_dedup','INSERT')
    AND has_table_privilege(current_user,'public.wallet_alert_dedup','DELETE')
    AND has_table_privilege(current_user,'public.wallet_alert_dedup','TRUNCATE')
    AND has_table_privilege(current_user,'public.user_wallets','SELECT')
    AND has_table_privilege(current_user,'public.user_wallets','INSERT')
    AND has_table_privilege(current_user,'public.user_wallets','UPDATE')
    AND has_table_privilege(current_user,'public.user_wallets','DELETE')
    AND has_table_privilege(current_user,'public.user_wallets','TRUNCATE')
    AND has_table_privilege(current_user,'public.wallet_seen_utxos','SELECT')
    AND has_table_privilege(current_user,'public.wallet_seen_utxos','INSERT')
    AND has_table_privilege(current_user,'public.wallet_seen_utxos','UPDATE')
    AND has_table_privilege(current_user,'public.wallet_seen_utxos','DELETE')
    AND has_table_privilege(current_user,'public.system_settings','SELECT')
    AND has_table_privilege(current_user,'public.system_settings','INSERT')
    AND has_table_privilege(current_user,'public.system_settings','UPDATE');
"#;
    let result = process::capture(
        "psql",
        [&runtime, "-v", "ON_ERROR_STOP=1", "-Atqc", verify_sql],
    )?;
    ensure!(
        result.trim() == "t",
        "CI PostgreSQL runtime privilege matrix failed"
    );
    println!("ci-postgres-prepare: PASS");
    Ok(())
}

pub fn admin_webhook_hardening() -> Result<()> {
    let checks: [(&str, &[&str]); 3] = [
        (
            "src/presentation/telegram/handlers/admin_confirm.rs",
            &[
                "ADMIN_CONFIRM_TTL_SECS",
                "MuteAlerts",
                "UnmuteAlerts",
                "validate_admin_do_callback",
                "cleanup_expired",
            ],
        ),
        (
            "src/infrastructure/webhook_security.rs",
            &[
                "WEBHOOK_SECRET_TOKEN",
                "WEBHOOK_BIND",
                "WEBHOOK_MAX_CONNECTIONS",
                "HEALTH_BIND",
                "metrics",
            ],
        ),
        (
            "src/presentation/telegram/commands.rs",
            &["mute_alerts", "unmute_alerts", "alerts_status"],
        ),
    ];
    ensure!(
        Path::new("src/presentation/telegram/menus.rs").is_file(),
        "missing required file: src/presentation/telegram/menus.rs"
    );
    for (file, terms) in checks {
        let text =
            fs::read_to_string(file).with_context(|| format!("missing required file: {file}"))?;
        for term in terms {
            ensure!(
                text.contains(term),
                "{file} missing required hardening marker: {term}"
            );
        }
    }
    println!("admin-webhook-hardening: PASS");
    Ok(())
}

pub fn rust_hardening() -> Result<()> {
    process::run(
        "cargo",
        [
            "clippy",
            "--locked",
            "-p",
            "kaspa-pulse",
            "--lib",
            "--bins",
            "--",
            "-D",
            "warnings",
            "-D",
            "clippy::unwrap_used",
            "-D",
            "clippy::expect_used",
        ],
    )?;
    println!("rust-hardening: PASS");
    Ok(())
}

fn allowed_placeholder(secret: &str) -> bool {
    [
        "PUT_",
        "YOUR_",
        "APP_PASSWORD",
        "TEST_PASSWORD",
        "POSTGRES_TEST_PASSWORD",
        "password",
        "example",
        "changeme",
        "REDACTED",
        "****",
        "RANDOM_",
        "SECRET_",
        "TEST_",
    ]
    .iter()
    .any(|prefix| secret.starts_with(prefix))
}

pub fn secret_scan() -> Result<()> {
    let excluded: BTreeSet<&str> = [
        ".env",
        ".env.example",
        ".gitignore",
        "README.md",
        "SECURITY_ADVISORIES.md",
    ]
    .into_iter()
    .collect();
    let bot = Regex::new(r#"BOT_TOKEN\s*[:=]\s*["']?([0-9]{7,}:[A-Za-z0-9_-]{30,})"#)?;
    let db = Regex::new(r#"DATABASE_URL\s*[:=]\s*["']?postgres(?:ql)?://[^:\s]+:([^@\s"']+)@"#)?;
    let superuser = Regex::new(r#"postgres(?:ql)?://postgres:([^@\s"']+)@"#)?;
    let webhook = Regex::new(r#"WEBHOOK_SECRET_TOKEN\s*[:=]\s*["']?([A-Za-z0-9_-]{32,})"#)?;
    let private_key = Regex::new(r"-----BEGIN (?:RSA |OPENSSH |EC |PRIVATE )?PRIVATE KEY-----")?;

    let mut files = Vec::new();
    walk_files(Path::new("."), &mut files)?;
    let mut findings = Vec::new();
    for path in files {
        let relative = path.strip_prefix("./").unwrap_or(&path);
        let rel = relative.to_string_lossy().replace('\\', "/");
        if excluded.contains(rel.as_str()) {
            continue;
        }
        if rel.starts_with(".backup/")
            || rel.starts_with("backups/")
            || rel.starts_with("local-cleanup-backup-")
        {
            findings.push(format!("sensitive backup path present: {rel}"));
            continue;
        }
        let file_name = relative
            .file_name()
            .and_then(|x| x.to_str())
            .unwrap_or_default();
        if (file_name.starts_with("project_code_export_") && file_name.ends_with(".txt"))
            || (file_name.starts_with("repo-before-history-clean-")
                && file_name.ends_with(".bundle"))
            || file_name.ends_with(".dump")
            || file_name.ends_with(".sql.dump")
            || file_name.ends_with(".bak")
        {
            findings.push(format!("sensitive backup/export artifact present: {rel}"));
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for captures in bot.captures_iter(&text) {
            let value = captures.get(1).map(|x| x.as_str()).unwrap_or_default();
            let token_part = value
                .split_once(':')
                .map(|(_, token)| token)
                .unwrap_or(value);
            if !allowed_placeholder(token_part) {
                findings.push(format!("real-looking Telegram BOT_TOKEN found in {rel}"));
            }
        }
        for captures in db.captures_iter(&text) {
            let value = captures.get(1).map(|x| x.as_str()).unwrap_or_default();
            if !allowed_placeholder(value) {
                findings.push(format!("real-looking DATABASE_URL password found in {rel}"));
            }
        }
        for captures in superuser.captures_iter(&text) {
            let value = captures.get(1).map(|x| x.as_str()).unwrap_or_default();
            if !allowed_placeholder(value) {
                findings.push(format!(
                    "real-looking postgres superuser URL found in {rel}"
                ));
            }
        }
        for captures in webhook.captures_iter(&text) {
            let value = captures.get(1).map(|x| x.as_str()).unwrap_or_default();
            if !allowed_placeholder(value) {
                findings.push(format!("real-looking WEBHOOK_SECRET_TOKEN found in {rel}"));
            }
        }
        if private_key.is_match(&text) {
            findings.push(format!("private key block found in {rel}"));
        }
    }
    findings.sort();
    findings.dedup();
    ensure!(
        findings.is_empty(),
        "secret scan findings:\n{}",
        findings.join("\n")
    );
    println!("secret-scan: PASS");
    Ok(())
}

pub fn security_pipeline() -> Result<()> {
    process::run("cargo", ["audit"])?;
    process::run("cargo", ["deny", "check"])?;
    process::run("cargo", ["tree", "--locked", "-d"])?;
    process::run("cargo", ["machete"])?;
    process::run(
        "cargo",
        [
            "clippy",
            "--locked",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ],
    )?;
    process::run(
        "cargo",
        [
            "test",
            "--locked",
            "--workspace",
            "--all-targets",
            "--all-features",
        ],
    )?;
    secret_scan()?;
    admin_webhook_hardening()?;
    rust_hardening()?;
    println!("security-pipeline: PASS");
    Ok(())
}

pub fn clean_history(confirm: &str, push: bool) -> Result<()> {
    ensure!(
        confirm == "I_UNDERSTAND_HISTORY_REWRITE",
        "refusing history rewrite without --confirm I_UNDERSTAND_HISTORY_REWRITE"
    );
    ensure!(
        Path::new("Cargo.toml").is_file(),
        "run from repository root"
    );
    let bundle = format!(
        "repo-before-history-clean-{}.bundle",
        Utc::now().format("%Y%m%d-%H%M%S")
    );
    process::run("git", ["bundle", "create", &bundle, "--all"])?;
    process::run(
        "git",
        [
            "filter-branch",
            "--force",
            "--index-filter",
            "git rm -r --cached --ignore-unmatch .backup backups *.dump",
            "--prune-empty",
            "--tag-name-filter",
            "cat",
            "--",
            "--all",
        ],
    )?;
    process::run("git", ["reflog", "expire", "--expire=now", "--all"])?;
    process::run("git", ["gc", "--prune=now", "--aggressive"])?;
    if push {
        process::run("git", ["push", "origin", "main", "--force-with-lease"])?;
    }
    println!("history-cleanup: PASS backup={bundle} pushed={push}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_are_distinguished_from_real_values() {
        assert!(allowed_placeholder("TEST_PASSWORD"));
        assert!(allowed_placeholder("REDACTED"));
        assert!(!allowed_placeholder("s3cretValue"));
    }
}
