mod cleanup;
mod evidence;
mod impact;
mod runtime;
mod scenarios;

use anyhow::{Context, Result, bail, ensure};
use chrono::Utc;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    env,
    ffi::OsStr,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

const TASK_ID: &str = "kaspa-telegram-opqual-v1";
pub(crate) const EVIDENCE_SCHEMA_VERSION: &str = "2.0.0";
const DOCKER_NETWORK: &str = "kp-opqual-v1";
const POSTGRES_CONTAINER: &str = "kp-opqual-postgres18";
const POSTGRES_VOLUME: &str = "kp-opqual-v1-pgdata";
const TELEGRAM_CONTAINER: &str = "kp-opqual-telegram";
const HTTP_CONTAINER: &str = "kp-opqual-http";
const NODE_CONTAINER: &str = "kp-opqual-node";
const APP_CONTAINER: &str = "kp-opqual-app";
const WEBHOOK_CONTAINER: &str = "kp-opqual-webhook-app";
const LOCK_CONTAINER: &str = "kp-opqual-lock";
const DATABASE: &str = "kaspa_opqual_v1";
const PG_ADMIN: &str = "opqual_admin";
const RUNTIME_ROLE: &str = "kaspa_pulse_app";
const OBSERVER_ROLE: &str = "opqual_observer";
const APPLICATION_NAME: &str = "kaspa-opqual-v1";
const WEBHOOK_APPLICATION_NAME: &str = "kaspa-opqual-v1-webhook";
const OBSERVER_APP_NAME: &str = "kaspa-opqual-observer";
const LOCK_APP_NAME: &str = "kaspa-opqual-lock-holder";
const POSTGRES_IMAGE: &str = "postgres:18";
const EXPECTED_POSTGRES_IMAGE_ID: &str =
    "sha256:4ef4dbc939d61acea57712655ddb4b4ab27419c913f94cca0cd57cb3ea3c2280";
const HEALTH_PORT: u16 = 18480;
const WEBHOOK_PORT: u16 = 18443;
const WEBHOOK_HEALTH_PORT: u16 = 18481;
const NODE_PORT: u16 = 17110;
const SYNTH_ADMIN_ID: i64 = 969100001;
const SYNTH_USER_ID: i64 = 969100002;
const SYNTH_CHAT_ID: i64 = 969100002;
const SYNTH_WALLET: &str = "kaspa:qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqkx9awp4e";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mode {
    DryRun,
    Run,
    Resume,
}

#[derive(Debug)]
pub(crate) struct ContextState {
    pub repo: PathBuf,
    pub run_id: String,
    pub run_dir: PathBuf,
    pub state_file: PathBuf,
    pub operations_file: PathBuf,
    pub control_dir: PathBuf,
    pub events_dir: PathBuf,
    pub certs_dir: PathBuf,
    pub logs_dir: PathBuf,
    pub receipts_dir: PathBuf,
    pub binary: PathBuf,
    pub binary_sha256: String,
    pub fixture_binary: PathBuf,
    pub fixture_sha256: String,
    pub source_head: String,
    pub source_tree: String,
    pub cargo_lock_sha256: String,
    pub harness_cargo_lock_sha256: String,
    pub scenario_map_sha256: String,
    pub harness_source_root: PathBuf,
    pub harness_source_hash: String,
    pub harness_binary_sha256: String,
    pub target_triple: String,
    pub git_event: String,
    pub tested_ref_kind: String,
    pub pr_head_sha: Option<String>,
    pub pr_head_tree: Option<String>,
    pub base_sha: Option<String>,
    pub base_tree: Option<String>,
    pub merge_base_sha: Option<String>,
    pub mode: Mode,
}

impl ContextState {
    fn create(mode: Mode, binary: &Path, resume_id: Option<&str>) -> Result<Self> {
        let repo = env::current_dir()?.canonicalize()?;
        ensure!(
            repo.join("Cargo.toml").is_file(),
            "run xtask from repository root"
        );
        let actual_head = capture("git", ["rev-parse", "HEAD"])?.trim().to_owned();
        let source_head = env::var("OPQUAL_TESTED_SHA").unwrap_or_else(|_| actual_head.clone());
        let actual_tree = capture("git", ["rev-parse", "HEAD^{tree}"])?
            .trim()
            .to_owned();
        let source_tree = env::var("OPQUAL_TESTED_TREE").unwrap_or_else(|_| actual_tree.clone());
        validate_tested_identity(&actual_head, &actual_tree, &source_head, &source_tree)?;
        let binary = binary
            .canonicalize()
            .with_context(|| format!("candidate binary missing: {}", binary.display()))?;
        let binary_sha256 = sha256_file(&binary)?;
        let harness_source_root = env::var_os("OPQUAL_HARNESS_SOURCE_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| repo.clone())
            .canonicalize()
            .context("canonicalize OPQUAL_HARNESS_SOURCE_ROOT")?;
        let fixture_binary = env::var_os("OPQUAL_FIXTURE_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(|| harness_source_root.join("target/release/opqual-fixture"));
        ensure!(
            fixture_binary.is_file(),
            "release opqual fixture missing: {}",
            fixture_binary.display()
        );
        let fixture_binary = fixture_binary.canonicalize()?;
        let fixture_sha256 = sha256_file(&fixture_binary)?;
        let cargo_lock_sha256 = sha256_file(&repo.join("Cargo.lock"))?;
        let harness_cargo_lock_sha256 = sha256_file(&harness_source_root.join("Cargo.lock"))?;
        let scenario_map_sha256 = sha256_file(&repo.join("opqual/scenario-map.csv"))?;
        let harness_source_hash = harness_source_hash(&harness_source_root)?;
        let harness_binary_sha256 = sha256_file(&env::current_exe()?)?;
        let target_triple = host_target_triple()?;
        let git_event = env::var("GITHUB_EVENT_NAME").unwrap_or_else(|_| "local".into());
        let tested_ref_kind =
            env::var("OPQUAL_TESTED_REF_KIND").unwrap_or_else(|_| "LOCAL_HEAD".into());
        let pr_head_sha = optional_env("OPQUAL_PR_HEAD_SHA");
        let pr_head_tree = optional_env("OPQUAL_PR_HEAD_TREE");
        let base_sha = optional_env("OPQUAL_BASE_SHA");
        let base_tree = optional_env("OPQUAL_BASE_TREE");
        let merge_base_sha = optional_env("OPQUAL_MERGE_BASE_SHA");
        let run_id = match (mode, resume_id) {
            (Mode::Resume, Some(id)) => id.to_owned(),
            (Mode::Resume, None) => bail!("resume requires run id"),
            (_, Some(_)) => bail!("run id is only valid for resume"),
            _ => format!(
                "{}-{}",
                Utc::now().format("%Y%m%dT%H%M%SZ"),
                std::process::id()
            ),
        };
        let run_dir = repo.join("evidence/opqual").join(&run_id);
        Ok(Self {
            state_file: run_dir.join("state.json"),
            operations_file: run_dir.join("operations.jsonl"),
            control_dir: run_dir.join("control"),
            events_dir: run_dir.join("events"),
            certs_dir: run_dir.join("certs"),
            logs_dir: run_dir.join("logs"),
            receipts_dir: run_dir.join("receipts"),
            repo,
            run_id,
            run_dir,
            binary,
            binary_sha256,
            fixture_binary,
            fixture_sha256,
            source_head,
            source_tree,
            cargo_lock_sha256,
            harness_cargo_lock_sha256,
            scenario_map_sha256,
            harness_source_root,
            harness_source_hash,
            harness_binary_sha256,
            target_triple,
            git_event,
            tested_ref_kind,
            pr_head_sha,
            pr_head_tree,
            base_sha,
            base_tree,
            merge_base_sha,
            mode,
        })
    }
    fn init(&self) -> Result<()> {
        for dir in [
            &self.run_dir,
            &self.control_dir,
            &self.events_dir,
            &self.certs_dir,
            &self.logs_dir,
            &self.receipts_dir,
        ] {
            fs::create_dir_all(dir)?;
        }
        if !self.state_file.exists() {
            atomic_json(&self.state_file, &json!({}))?;
        }
        if !self.operations_file.exists() {
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.operations_file)?;
        }
        Ok(())
    }
    fn state(&self) -> Result<Map<String, Value>> {
        let value: Value = serde_json::from_str(&fs::read_to_string(&self.state_file)?)?;
        Ok(value.as_object().cloned().unwrap_or_default())
    }
    fn state_set(&self, key: &str, value: impl Into<Value>) -> Result<()> {
        let mut state = self.state()?;
        state.insert(key.to_owned(), value.into());
        atomic_json(&self.state_file, &Value::Object(state))
    }
    fn state_is(&self, key: &str, expected: &str) -> Result<bool> {
        Ok(self.state()?.get(key).and_then(Value::as_str) == Some(expected))
    }
    fn mark_phase(&self, phase: &str, value: &str) -> Result<()> {
        self.state_set(&format!("phase.{phase}"), value)
    }
    fn require_phase(&self, phase: &str) -> Result<()> {
        ensure!(
            self.state_is(&format!("phase.{phase}"), "VERIFIED")?,
            "required phase not verified: {phase}"
        );
        Ok(())
    }
    fn record(&self, op: &str, phase: &str, detail: &str) -> Result<()> {
        let rec = json!({
            "op": op, "task": TASK_ID, "phase": phase,
            "utc": Utc::now().to_rfc3339(), "host": hostname(),
            "run_id": self.run_id, "detail": detail
        });
        append_jsonl(&self.operations_file, &rec)
    }
    fn dry(&self) -> bool {
        self.mode == Mode::DryRun
    }
}

fn validate_tested_identity(
    actual_head: &str,
    actual_tree: &str,
    tested_sha: &str,
    tested_tree: &str,
) -> Result<()> {
    ensure!(
        actual_head == tested_sha,
        "working repository HEAD {} does not equal TESTED_SHA {}",
        actual_head,
        tested_sha
    );
    ensure!(
        actual_tree == tested_tree,
        "working repository tree {} does not equal TESTED_TREE {}",
        actual_tree,
        tested_tree
    );
    Ok(())
}

fn optional_env(key: &str) -> Option<String> {
    env::var(key).ok().filter(|value| !value.trim().is_empty())
}

fn host_target_triple() -> Result<String> {
    let verbose = capture("rustc", ["-vV"])?;
    verbose
        .lines()
        .find_map(|line| line.strip_prefix("host: ").map(str::to_owned))
        .context("rustc -vV did not report host target")
}

fn harness_source_hash(root: &Path) -> Result<String> {
    let mut files = vec![
        root.join("xtask/Cargo.toml"),
        root.join("xtask/src/main.rs"),
        root.join("xtask/src/opqual.rs"),
    ];
    let opqual_dir = root.join("xtask/src/opqual");
    if opqual_dir.is_dir() {
        for entry in fs::read_dir(&opqual_dir)? {
            let path = entry?.path();
            if path.is_file() {
                files.push(path);
            }
        }
    }
    files.sort();
    let mut hasher = Sha256::new();
    for path in files {
        let rel = path.strip_prefix(root).with_context(|| {
            format!(
                "harness source {} is outside {}",
                path.display(),
                root.display()
            )
        })?;
        let rel = rel
            .to_str()
            .context("fixed harness source path must be valid UTF-8")?;
        hasher.update((rel.len() as u64).to_be_bytes());
        hasher.update(rel.as_bytes());
        let bytes =
            fs::read(&path).with_context(|| format!("read harness source {}", path.display()))?;
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(&bytes);
    }
    Ok(hex_sha256(hasher.finalize().as_slice()))
}

pub(crate) fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_sha256(hasher.finalize().as_slice())
}

fn hex_sha256(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn canonical_json(value: &Value) -> Result<Vec<u8>> {
    Ok(serde_json_canonicalizer::to_vec(value)?)
}

pub fn impact(
    base_sha: &str,
    tested_sha: &str,
    baseline_proof: Option<&Path>,
    output: &Path,
) -> Result<()> {
    impact::execute(base_sha, tested_sha, baseline_proof, output)
}

pub fn execute(mode: Mode, binary: &Path, resume_id: Option<&str>) -> Result<()> {
    let ctx = ContextState::create(mode, binary, resume_id)?;
    ctx.init()?;
    if mode == Mode::Resume && ctx.state_is("cleanup.completed", "true")? {
        bail!(
            "run {} is already cleaned/closed; start a new run",
            ctx.run_id
        );
    }
    preflight(&ctx)?;
    if ctx.dry() {
        println!("OPQUAL_DRY_RUN=PASS run_id={}", ctx.run_id);
        return Ok(());
    }
    let lifecycle = (|| -> Result<()> {
        provision(&ctx)?;
        migrate(&ctx)?;
        runtime::fixtures(&ctx)?;
        runtime::start_app(&ctx)?;
        runtime::create_lock(&ctx)?;
        runtime::exec01(&ctx)?;
        runtime::restart(&ctx)?;
        scenarios::execute(&ctx)?;
        Ok(())
    })();
    let cleanup_result = cleanup::execute(&ctx);
    let evidence_result = evidence::collect(&ctx);
    lifecycle?;
    cleanup_result?;
    evidence_result?;
    println!(
        "OPQUAL_COMPLETE run_id={} source={} binary_sha256={} fixture_sha256={}",
        ctx.run_id, ctx.source_head, ctx.binary_sha256, ctx.fixture_sha256
    );
    println!("OPQUAL_TESTED_TREE={}", ctx.source_tree);
    println!("OPQUAL_HARNESS_SOURCE_HASH={}", ctx.harness_source_hash);
    Ok(())
}

fn contract_manifest(ctx: &ContextState) -> Value {
    json!({
        "task_id": TASK_ID,
        "evidence_schema_version": EVIDENCE_SCHEMA_VERSION,
        "repository_root": ctx.repo,
        "identity": {
            "git_event": ctx.git_event,
            "tested_ref_kind": ctx.tested_ref_kind,
            "tested_sha": ctx.source_head,
            "tested_tree": ctx.source_tree,
            "source_head": ctx.source_head,
            "source_tree": ctx.source_tree,
            "pr_head_sha": ctx.pr_head_sha,
            "pr_head_tree": ctx.pr_head_tree,
            "base_sha": ctx.base_sha,
            "base_tree": ctx.base_tree,
            "merge_base_sha": ctx.merge_base_sha,
            "application_binary_sha256": ctx.binary_sha256,
            "fixture_binary_sha256": ctx.fixture_sha256,
            "cargo_lock_sha256": ctx.cargo_lock_sha256,
            "harness_cargo_lock_sha256": ctx.harness_cargo_lock_sha256,
            "scenario_map_sha256": ctx.scenario_map_sha256,
            "harness_source_root": ctx.harness_source_root,
            "harness_source_hash": ctx.harness_source_hash,
            "harness_binary_sha256": ctx.harness_binary_sha256,
            "target_triple": ctx.target_triple
        },
        "docker": {
            "network": DOCKER_NETWORK,
            "postgres_container": POSTGRES_CONTAINER,
            "postgres_volume": POSTGRES_VOLUME,
            "telegram_container": TELEGRAM_CONTAINER,
            "http_container": HTTP_CONTAINER,
            "node_container": NODE_CONTAINER,
            "app_container": APP_CONTAINER,
            "webhook_container": WEBHOOK_CONTAINER,
            "lock_container": LOCK_CONTAINER,
            "postgres_image": POSTGRES_IMAGE,
            "postgres_image_digest": EXPECTED_POSTGRES_IMAGE_ID
        },
        "database": {
            "name": DATABASE,
            "admin_role": PG_ADMIN,
            "runtime_role": RUNTIME_ROLE,
            "observer_role": OBSERVER_ROLE,
            "application_name": APPLICATION_NAME,
            "webhook_application_name": WEBHOOK_APPLICATION_NAME,
            "observer_application_name": OBSERVER_APP_NAME,
            "lock_application_name": LOCK_APP_NAME
        },
        "ports": {
            "health": HEALTH_PORT,
            "webhook": WEBHOOK_PORT,
            "webhook_health": WEBHOOK_HEALTH_PORT,
            "node": NODE_PORT
        },
        "synthetic": {
            "admin_id": SYNTH_ADMIN_ID,
            "user_id": SYNTH_USER_ID,
            "chat_id": SYNTH_CHAT_ID,
            "wallet": SYNTH_WALLET
        }
    })
}

fn phase_current(ctx: &ContextState, phase: &str) -> Result<bool> {
    match phase {
        "provision" => Ok(network_exists(DOCKER_NETWORK)?
            && volume_exists(POSTGRES_VOLUME)?
            && container_exists(POSTGRES_CONTAINER)?
            && docker_inspect(POSTGRES_CONTAINER, "{{.State.Running}}")? == "true"),
        "migrate" => {
            if !container_exists(POSTGRES_CONTAINER)? {
                return Ok(false);
            }
            Ok(pg_admin(
                ctx,
                "SELECT to_regclass('public.opqual_harness_migrations') IS NOT NULL;",
            )?
            .trim()
                == "t")
        }
        "fixtures" => Ok([TELEGRAM_CONTAINER, HTTP_CONTAINER, NODE_CONTAINER]
            .iter()
            .all(|name| {
                container_exists(name).unwrap_or(false)
                    && docker_inspect(name, "{{.State.Running}}").unwrap_or_default() == "true"
            })),
        "start_app" => Ok(container_exists(APP_CONTAINER)?
            && docker_inspect(APP_CONTAINER, "{{.State.Running}}")? == "true"),
        _ => Ok(false),
    }
}

fn maybe_skip_verified(ctx: &ContextState, phase: &str) -> Result<bool> {
    if !ctx.state_is(&format!("phase.{phase}"), "VERIFIED")? {
        return Ok(false);
    }
    if phase_current(ctx, phase)? {
        ctx.record(
            &format!("RESUME:{phase}"),
            "VERIFIED",
            "phase receipt and current side effects agree; skipped replay",
        )?;
        return Ok(true);
    }
    ctx.record(
        &format!("RESUME:{phase}"),
        "UNKNOWN",
        "phase was VERIFIED but current side effects no longer match; refusing blind replay",
    )?;
    bail!("resume mismatch for phase {phase}; actual state must be reconciled before retry")
}

fn provision(ctx: &ContextState) -> Result<()> {
    ctx.require_phase("preflight")?;
    if maybe_skip_verified(ctx, "provision")? {
        return Ok(());
    }
    ctx.record(
        "PROVISION",
        "PLANNED",
        "create/reconcile internal task network, labelled pgdata volume and PostgreSQL 18",
    )?;
    if !network_exists(DOCKER_NETWORK)? {
        run(
            "sudo",
            [
                "-n",
                "docker",
                "network",
                "create",
                "--internal",
                "--label",
                &format!("task={TASK_ID}"),
                "--label",
                "purpose=opqual",
                DOCKER_NETWORK,
            ],
        )?;
    } else {
        assert_owned_network(DOCKER_NETWORK)?;
    }
    let internal = capture(
        "sudo",
        [
            "-n",
            "docker",
            "network",
            "inspect",
            DOCKER_NETWORK,
            "--format",
            "{{.Internal}}",
        ],
    )?;
    ensure!(
        internal.trim() == "true",
        "network is not internal: {DOCKER_NETWORK}"
    );
    if !volume_exists(POSTGRES_VOLUME)? {
        run(
            "sudo",
            [
                "-n",
                "docker",
                "volume",
                "create",
                "--label",
                &format!("task={TASK_ID}"),
                "--label",
                "purpose=postgres-data",
                POSTGRES_VOLUME,
            ],
        )?;
    } else {
        assert_owned_volume(POSTGRES_VOLUME)?;
    }
    if !container_exists(POSTGRES_CONTAINER)? {
        run(
            "sudo",
            [
                "-n",
                "docker",
                "run",
                "-d",
                "--name",
                POSTGRES_CONTAINER,
                "--network",
                DOCKER_NETWORK,
                "--network-alias",
                POSTGRES_CONTAINER,
                "--label",
                &format!("task={TASK_ID}"),
                "--label",
                "purpose=postgresql",
                "-e",
                &format!("POSTGRES_DB={DATABASE}"),
                "-e",
                &format!("POSTGRES_USER={PG_ADMIN}"),
                "-e",
                "POSTGRES_HOST_AUTH_METHOD=trust",
                "--mount",
                &format!("type=volume,src={POSTGRES_VOLUME},dst=/var/lib/postgresql"),
                POSTGRES_IMAGE,
            ],
        )?;
    } else {
        assert_owned_container(POSTGRES_CONTAINER)?;
        if docker_inspect(POSTGRES_CONTAINER, "{{.State.Running}}")? != "true" {
            run("sudo", ["-n", "docker", "start", POSTGRES_CONTAINER])?;
        }
    }
    assert_no_published_ports(POSTGRES_CONTAINER)?;
    wait_until(
        || {
            Ok(run_status(
                "sudo",
                [
                    "-n",
                    "docker",
                    "exec",
                    POSTGRES_CONTAINER,
                    "pg_isready",
                    "-U",
                    PG_ADMIN,
                    "-d",
                    DATABASE,
                ],
            )?
            .0)
        },
        60,
        Duration::from_secs(1),
        "PostgreSQL did not become ready",
    )?;
    let network_map = json!({
        "network": {
            "name": DOCKER_NETWORK,
            "internal": true,
            "task": TASK_ID
        },
        "postgres": {
            "name": POSTGRES_CONTAINER,
            "image": docker_inspect(POSTGRES_CONTAINER,"{{.Image}}")?,
            "running": docker_inspect(POSTGRES_CONTAINER,"{{.State.Running}}")?=="true",
            "port_bindings": docker_inspect(POSTGRES_CONTAINER,"{{json .HostConfig.PortBindings}}")?
        }
    });
    atomic_json(&ctx.run_dir.join("network-map.json"), &network_map)?;
    ctx.mark_phase("provision", "VERIFIED")?;
    ctx.record(
        "PROVISION",
        "VERIFIED",
        "internal-only PostgreSQL 18 environment ready; no host ports",
    )?;
    Ok(())
}

fn migration_files(ctx: &ContextState) -> Result<Vec<PathBuf>> {
    let mut files = fs::read_dir(ctx.repo.join("migrations"))?
        .filter_map(|e| e.ok().map(|x| x.path()))
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("sql"))
        .collect::<Vec<_>>();
    files.sort();
    ensure!(!files.is_empty(), "no repository migrations found");
    Ok(files)
}

fn migrate(ctx: &ContextState) -> Result<()> {
    ctx.require_phase("provision")?;
    if maybe_skip_verified(ctx, "migrate")? {
        return Ok(());
    }
    ctx.record("MIGRATE","PLANNED","create least-privilege runtime/observer roles and apply repository migrations with hash receipts")?;
    let roles = format!(
        r#"DO $$
BEGIN
 IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='{RUNTIME_ROLE}') THEN CREATE ROLE {RUNTIME_ROLE} LOGIN; END IF;
 IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname='{OBSERVER_ROLE}') THEN CREATE ROLE {OBSERVER_ROLE} LOGIN; END IF;
END
$$;
GRANT CONNECT ON DATABASE {DATABASE} TO {RUNTIME_ROLE}, {OBSERVER_ROLE};
GRANT pg_monitor TO {OBSERVER_ROLE};
CREATE TABLE IF NOT EXISTS public.opqual_harness_migrations (
 name text PRIMARY KEY, sha256 text NOT NULL, applied_at timestamptz NOT NULL DEFAULT clock_timestamp()
);"#
    );
    pg_admin_input(ctx, &roles)?;
    let mut receipt = Vec::new();
    for migration in migration_files(ctx)? {
        let name = migration
            .file_name()
            .and_then(|x| x.to_str())
            .context("migration filename")?
            .to_owned();
        let sha = sha256_file(&migration)?;
        let query = format!(
            "SELECT sha256 FROM public.opqual_harness_migrations WHERE name='{}';",
            sql_literal(&name)
        );
        let recorded = pg_admin(ctx, &query)?.trim().to_owned();
        if !recorded.is_empty() {
            ensure!(
                recorded == sha,
                "migration receipt hash mismatch for {name}"
            );
            receipt.push(json!({"name":name,"sha256":sha,"result":"SKIPPED_ALREADY_VERIFIED"}));
            continue;
        }
        ctx.record(&format!("MIGRATION:{name}"), "PLANNED", &sha)?;
        let sql = fs::read_to_string(&migration)?;
        let apply = format!(
            "BEGIN;\n{sql}\nINSERT INTO public.opqual_harness_migrations(name,sha256) VALUES ('{}','{}');\nCOMMIT;\n",
            sql_literal(&name),
            sha
        );
        pg_admin_input(ctx, &apply)?;
        let verify = pg_admin(ctx, &query)?.trim().to_owned();
        ensure!(
            verify == sha,
            "migration receipt verification failed for {name}"
        );
        receipt.push(json!({"name":name,"sha256":sha,"result":"APPLIED"}));
        ctx.record(&format!("MIGRATION:{name}"), "VERIFIED", &sha)?;
    }
    let verify = pg_admin(
        ctx,
        &format!(
            "SELECT (current_database()='{DATABASE}') AND (current_setting('server_version_num')::int/10000=18) AND to_regclass('public.user_wallets') IS NOT NULL AND to_regclass('public.telegram_delivery_queue') IS NOT NULL AND EXISTS(SELECT 1 FROM pg_roles WHERE rolname='{RUNTIME_ROLE}');"
        ),
    )?;
    ensure!(verify.trim() == "t", "schema/role verification failed");
    atomic_json(
        &ctx.run_dir.join("migration-receipt.json"),
        &json!({"source":"repository:migrations/","files":receipt,"duplicated_sql":false}),
    )?;
    ctx.mark_phase("migrate", "VERIFIED")?;
    ctx.record(
        "MIGRATE",
        "VERIFIED",
        "repository migrations and role/schema contract verified",
    )?;
    Ok(())
}

fn pg_admin(ctx: &ContextState, sql: &str) -> Result<String> {
    assert_owned_container(POSTGRES_CONTAINER)?;
    let _ = &ctx.repo;
    run_input(
        "sudo",
        [
            "-n",
            "docker",
            "exec",
            "-i",
            "-e",
            &format!("PGAPPNAME={OBSERVER_APP_NAME}"),
            POSTGRES_CONTAINER,
            "psql",
            "-X",
            "-v",
            "ON_ERROR_STOP=1",
            "-U",
            PG_ADMIN,
            "-d",
            DATABASE,
            "-At",
        ],
        sql,
    )
}
fn pg_admin_input(ctx: &ContextState, sql: &str) -> Result<()> {
    let _ = pg_admin(ctx, sql)?;
    Ok(())
}
fn sql_literal(value: &str) -> String {
    value.replace('\'', "''")
}
fn assert_no_published_ports(name: &str) -> Result<()> {
    assert_owned_container(name)?;
    let bindings = docker_inspect(name, "{{json .HostConfig.PortBindings}}")?;
    ensure!(
        bindings == "null" || bindings == "{}",
        "{name} publishes host ports: {bindings}"
    );
    Ok(())
}

fn preflight(ctx: &ContextState) -> Result<()> {
    atomic_json(&ctx.run_dir.join("contract.json"), &contract_manifest(ctx))?;
    ctx.record(
        "PREFLIGHT",
        "PLANNED",
        "verify host, exact candidate, Rust fixture, Docker isolation and ownership conflicts",
    )?;
    ensure!(
        hostname().split('.').next() == Some("kas"),
        "host must be kas"
    );
    ensure!(env::consts::OS == "linux", "Linux required");
    ensure!(env::consts::ARCH == "x86_64", "x86_64 required");
    for tool in ["git", "sudo", "docker"] {
        require(tool)?;
    }
    for key in [
        "BOT_TOKEN",
        "DATABASE_URL",
        "NODE_URL_01",
        "ADMIN_ID",
        "ADMIN_USER_ID",
        "ADMIN_CHAT_ID",
        "WEBHOOK_SECRET_TOKEN",
    ] {
        ensure!(
            env::var_os(key).is_none(),
            "host environment contains {key}; refusing inherited runtime credentials"
        );
    }
    ensure!(
        capture("git", ["rev-parse", "HEAD"])?.trim() == ctx.source_head,
        "TESTED_SHA changed during preflight"
    );
    ensure!(
        capture("git", ["rev-parse", "HEAD^{tree}"])?.trim() == ctx.source_tree,
        "TESTED_TREE changed during preflight"
    );
    ensure!(
        sha256_file(&ctx.repo.join("Cargo.lock"))? == ctx.cargo_lock_sha256,
        "tested Cargo.lock identity changed during preflight"
    );
    ensure!(
        sha256_file(&ctx.harness_source_root.join("Cargo.lock"))? == ctx.harness_cargo_lock_sha256,
        "harness Cargo.lock identity changed during preflight"
    );
    ensure!(
        sha256_file(&ctx.repo.join("opqual/scenario-map.csv"))? == ctx.scenario_map_sha256,
        "scenario-map identity changed during preflight"
    );
    ensure!(
        harness_source_hash(&ctx.harness_source_root)? == ctx.harness_source_hash,
        "harness source identity changed during preflight"
    );
    ensure!(
        sha256_file(&env::current_exe()?)? == ctx.harness_binary_sha256,
        "harness binary identity changed during preflight"
    );
    ensure!(
        sha256_file(&ctx.binary)? == ctx.binary_sha256,
        "qualified binary SHA mismatch"
    );
    ensure_binary_embeds_source_revision(&ctx.binary, &ctx.source_head)?;
    ensure!(
        sha256_file(&ctx.fixture_binary)? == ctx.fixture_sha256,
        "Rust fixture binary SHA mismatch"
    );
    let image_id = capture_status(
        "sudo",
        [
            "-n",
            "docker",
            "image",
            "inspect",
            POSTGRES_IMAGE,
            "--format",
            "{{.Id}}",
        ],
    )?;
    ensure!(image_id.0, "postgres:18 image missing");
    ensure!(
        image_id.1.trim() == EXPECTED_POSTGRES_IMAGE_ID,
        "postgres:18 image mismatch: {}",
        image_id.1.trim()
    );
    for name in [
        POSTGRES_CONTAINER,
        TELEGRAM_CONTAINER,
        HTTP_CONTAINER,
        NODE_CONTAINER,
        APP_CONTAINER,
        LOCK_CONTAINER,
        WEBHOOK_CONTAINER,
    ] {
        if container_exists(name)? {
            assert_owned_container(name)?;
        }
    }
    if network_exists(DOCKER_NETWORK)? {
        assert_owned_network(DOCKER_NETWORK)?;
    }
    if volume_exists(POSTGRES_VOLUME)? {
        assert_owned_volume(POSTGRES_VOLUME)?;
    }
    if ctx.dry() {
        ctx.mark_phase("preflight", "VERIFIED")?;
        ctx.state_set("dry_run", "true")?;
        ctx.record(
            "PREFLIGHT",
            "VERIFIED",
            "dry-run only; no Docker/database/signal mutation",
        )?;
        println!(
            "OPQUAL PLAN: preflight -> provision -> migrate -> fixtures -> start-app -> create-lock -> EXEC-01 -> restart -> scenarios -> cleanup -> evidence"
        );
    } else {
        ctx.mark_phase("preflight", "VERIFIED")?;
        ctx.record(
            "PREFLIGHT",
            "VERIFIED",
            "candidate/source/binary/Rust-fixture/tool/isolation prerequisites satisfied",
        )?;
    }
    Ok(())
}

pub(crate) fn run_capture<I, S>(program: &str, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let out = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("execute {program}"))?;
    if !out.status.success() {
        bail!("{program} failed: {}", String::from_utf8_lossy(&out.stderr));
    }
    Ok(String::from_utf8(out.stdout)?)
}
fn capture<I, S>(program: &str, args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    run_capture(program, args)
}
pub(crate) fn run_status<I, S>(program: &str, args: I) -> Result<(bool, String)>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let out = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("execute {program}"))?;
    Ok((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    ))
}
fn capture_status<I, S>(program: &str, args: I) -> Result<(bool, String)>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    run_status(program, args)
}
pub(crate) fn run<I, S>(program: &str, args: I) -> Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let st = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("execute {program}"))?;
    ensure!(st.success(), "{program} failed with {st}");
    Ok(())
}
pub(crate) fn run_input<I, S>(program: &str, args: I, input: &str) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .as_mut()
        .context("stdin unavailable")?
        .write_all(input.as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!("{program} failed: {}", String::from_utf8_lossy(&out.stderr))
    }
    Ok(String::from_utf8(out.stdout)?)
}
fn require(program: &str) -> Result<()> {
    let ok = Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    ensure!(ok, "required command unavailable: {program}");
    Ok(())
}
fn hostname() -> String {
    fs::read_to_string("/etc/hostname")
        .unwrap_or_else(|_| "unknown".into())
        .trim()
        .to_owned()
}
fn ensure_binary_embeds_source_revision(path: &Path, expected_sha: &str) -> Result<()> {
    ensure!(
        expected_sha.len() == 40 && expected_sha.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "TESTED_SHA must be a full 40-character hexadecimal commit id"
    );
    let bytes =
        fs::read(path).with_context(|| format!("read candidate binary {}", path.display()))?;
    ensure!(
        bytes
            .windows(expected_sha.len())
            .any(|window| window == expected_sha.as_bytes()),
        "candidate binary does not embed TESTED_SHA {}; build with KASPA_PULSE_SOURCE_REVISION={}",
        expected_sha,
        expected_sha
    );
    Ok(())
}

pub(crate) fn sha256_file(path: &Path) -> Result<String> {
    let mut f = fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    let digest = h.finalize();
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}
pub(crate) fn atomic_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|x| x.to_str()).unwrap_or("bin")
    ));
    let mut f = fs::File::create(&tmp)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    fs::rename(&tmp, path)?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    ensure!(fs::read(path)? == bytes, "atomic byte readback mismatch");
    Ok(())
}

fn atomic_json(path: &Path, value: &Value) -> Result<()> {
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|x| x.to_str()).unwrap_or("json")
    ));
    let mut f = fs::File::create(&tmp)?;
    serde_json::to_writer_pretty(&mut f, value)?;
    writeln!(f)?;
    f.sync_all()?;
    fs::rename(&tmp, path)?;
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?
    }
    let check: Value = serde_json::from_str(&fs::read_to_string(path)?)?;
    ensure!(&check == value, "atomic JSON readback mismatch");
    Ok(())
}
pub(crate) fn append_jsonl(path: &Path, value: &Value) -> Result<()> {
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{}", serde_json::to_string(value)?)?;
    f.sync_data()?;
    Ok(())
}
fn docker_inspect(name: &str, format: &str) -> Result<String> {
    Ok(capture(
        "sudo",
        ["-n", "docker", "inspect", name, "--format", format],
    )?
    .trim()
    .to_owned())
}
fn container_exists(name: &str) -> Result<bool> {
    Ok(run_status("sudo", ["-n", "docker", "container", "inspect", name])?.0)
}
fn network_exists(name: &str) -> Result<bool> {
    Ok(run_status("sudo", ["-n", "docker", "network", "inspect", name])?.0)
}
fn volume_exists(name: &str) -> Result<bool> {
    Ok(run_status("sudo", ["-n", "docker", "volume", "inspect", name])?.0)
}
fn assert_owned_container(name: &str) -> Result<()> {
    ensure!(container_exists(name)?, "container missing: {name}");
    ensure!(
        docker_inspect(name, "{{ index .Config.Labels \"task\" }}")? == TASK_ID,
        "refusing non-owned container: {name}"
    );
    Ok(())
}
fn assert_owned_network(name: &str) -> Result<()> {
    ensure!(network_exists(name)?, "network missing: {name}");
    let label = capture(
        "sudo",
        [
            "-n",
            "docker",
            "network",
            "inspect",
            name,
            "--format",
            "{{ index .Labels \"task\" }}",
        ],
    )?;
    ensure!(
        label.trim() == TASK_ID,
        "refusing non-owned network: {name}"
    );
    Ok(())
}
fn assert_owned_volume(name: &str) -> Result<()> {
    ensure!(volume_exists(name)?, "volume missing: {name}");
    let label = capture(
        "sudo",
        [
            "-n",
            "docker",
            "volume",
            "inspect",
            name,
            "--format",
            "{{ index .Labels \"task\" }}",
        ],
    )?;
    ensure!(label.trim() == TASK_ID, "refusing non-owned volume: {name}");
    Ok(())
}
pub(crate) fn wait_until(
    mut f: impl FnMut() -> Result<bool>,
    tries: usize,
    delay: Duration,
    msg: &str,
) -> Result<()> {
    for _ in 0..tries {
        if f()? {
            return Ok(());
        }
        thread::sleep(delay)
    }
    bail!("{msg}")
}

#[cfg(test)]
mod evidence_identity_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tested_identity_requires_exact_sha_and_tree() {
        validate_tested_identity("a", "t1", "a", "t1").unwrap();
        assert!(validate_tested_identity("a", "t1", "b", "t1").is_err());
        assert!(validate_tested_identity("a", "t1", "a", "t2").is_err());
    }

    #[test]
    fn canonical_json_uses_rfc8785_order_and_number_form() {
        let value = json!({"b":false,"c":120.0,"a":"Hello!"});
        let canonical = String::from_utf8(canonical_json(&value).unwrap()).unwrap();
        assert_eq!(canonical, r#"{"a":"Hello!","b":false,"c":120}"#);
    }

    #[test]
    fn candidate_binary_must_embed_tested_sha() {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("candidate");
        let sha = "0123456789abcdef0123456789abcdef01234567";
        fs::write(&binary, format!("prefix-{sha}-suffix")).unwrap();
        ensure_binary_embeds_source_revision(&binary, sha).unwrap();
        assert!(
            ensure_binary_embeds_source_revision(
                &binary,
                "89abcdef0123456789abcdef0123456789abcdef"
            )
            .is_err()
        );
        assert!(ensure_binary_embeds_source_revision(&binary, "short").is_err());
    }

    #[test]
    fn canonical_hash_is_independent_of_object_insertion_order() {
        let left: Value = serde_json::from_str(r#"{"z":1,"a":{"y":2,"b":3}}"#).unwrap();
        let right: Value = serde_json::from_str(r#"{"a":{"b":3,"y":2},"z":1}"#).unwrap();
        assert_eq!(
            canonical_json(&left).unwrap(),
            canonical_json(&right).unwrap()
        );
        assert_eq!(
            sha256_bytes(&canonical_json(&left).unwrap()),
            sha256_bytes(&canonical_json(&right).unwrap())
        );
    }
}
