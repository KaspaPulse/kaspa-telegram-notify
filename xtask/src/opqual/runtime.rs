use super::*;
use anyhow::{Context, Result, ensure};
use serde_json::json;
use std::{fs, time::Duration};

pub(super) fn fixtures(ctx: &ContextState) -> Result<()> {
    ctx.require_phase("migrate")?;
    if maybe_skip_verified(ctx, "fixtures")? {
        return Ok(());
    }
    ctx.record(
        "FIXTURES",
        "PLANNED",
        "create task CA and Rust-native synthetic providers",
    )?;
    ensure_certificates(ctx)?;
    fs::write(ctx.control_dir.join("telegram.json"), "{}\n")?;
    fs::write(ctx.control_dir.join("http.json"), "{}\n")?;
    fs::write(ctx.control_dir.join("kaspa.json"), "{}\n")?;
    let updates = ctx.control_dir.join("telegram-updates.jsonl");
    if !updates.exists() {
        fs::write(&updates, "")?;
    }
    start_fixture(
        ctx,
        TELEGRAM_CONTAINER,
        "telegram",
        "telegram",
        &["--network-alias", "api.telegram.org"],
    )?;
    start_fixture(
        ctx,
        HTTP_CONTAINER,
        "http-providers",
        "http",
        &[
            "--network-alias",
            "api.kaspa.org",
            "--network-alias",
            "api.coingecko.com",
        ],
    )?;
    start_fixture(
        ctx,
        NODE_CONTAINER,
        "kaspa-wrpc",
        "kaspa",
        &["--network-alias", NODE_CONTAINER],
    )?;
    for name in [TELEGRAM_CONTAINER, HTTP_CONTAINER, NODE_CONTAINER] {
        wait_until(
            || Ok(container_exists(name)? && docker_inspect(name, "{{.State.Running}}")? == "true"),
            40,
            Duration::from_millis(250),
            &format!("fixture failed to stay running: {name}"),
        )?;
        assert_no_published_ports(name)?;
    }
    for url in [
        "https://api.telegram.org/bot1234567890:TEST_TOKEN_NOT_REAL/getMe",
        "https://api.kaspa.org/info/price",
        "https://api.kaspa.org/info/marketcap",
        "https://api.kaspa.org/info/fee-estimate",
        "https://api.coingecko.com/api/v3/simple/price?ids=kaspa&vs_currencies=usd&include_market_cap=true",
    ] {
        probe(ctx, url, true)?;
    }
    probe(ctx, &format!("http://{NODE_CONTAINER}:{NODE_PORT}/"), false)?;
    pg_admin_input(
        ctx,
        &format!(
            r#"
INSERT INTO system_settings(key_name,value_data) VALUES
 ('ENABLE_MEMORY_CLEANER','false'),('ENABLE_LIVE_SYNC','true'),('MAINTENANCE_MODE','false')
ON CONFLICT(key_name) DO UPDATE SET value_data=EXCLUDED.value_data;
INSERT INTO user_wallets(wallet,chat_id) VALUES ('{}',{SYNTH_CHAT_ID})
ON CONFLICT(wallet,chat_id) DO NOTHING;
INSERT INTO kas_price_history(day,price_usd,source)
SELECT d::date,0.123456,'opqual-synthetic'
FROM generate_series(CURRENT_DATE-100,CURRENT_DATE,interval '1 day') d
ON CONFLICT(day) DO NOTHING;
"#,
            sql_literal(SYNTH_WALLET)
        ),
    )?;
    atomic_json(
        &ctx.run_dir.join("fixture-hashes.json"),
        &json!({"opqual-fixture":ctx.fixture_sha256,"modes":["telegram","http","kaspa","probe"]}),
    )?;
    atomic_json(
        &ctx.run_dir.join("synthetic-identities.json"),
        &json!({
            "admin_user_id":SYNTH_ADMIN_ID,"ordinary_user_id":SYNTH_USER_ID,
            "ordinary_chat_id":SYNTH_CHAT_ID,"wallet":SYNTH_WALLET,"real_identity":false
        }),
    )?;
    ctx.mark_phase("fixtures", "VERIFIED")?;
    ctx.record(
        "FIXTURES",
        "VERIFIED",
        "Rust-native synthetic providers ready; no public listeners",
    )?;
    Ok(())
}

fn ensure_certificates(ctx: &ContextState) -> Result<()> {
    let ca_key = ctx.certs_dir.join("ca.key");
    let ca_crt = ctx.certs_dir.join("ca.crt");
    let key = ctx.certs_dir.join("server.key");
    let csr = ctx.certs_dir.join("server.csr");
    let ext = ctx.certs_dir.join("server.ext");
    let crt = ctx.certs_dir.join("server.crt");
    if ca_crt.is_file() && crt.is_file() && key.is_file() {
        return Ok(());
    }
    run(
        "openssl",
        [
            "genrsa",
            "-out",
            ca_key.to_str().context("ca key path")?,
            "2048",
        ],
    )?;
    run(
        "openssl",
        [
            "req",
            "-x509",
            "-new",
            "-sha256",
            "-days",
            "2",
            "-key",
            ca_key.to_str().context("ca key path")?,
            "-subj",
            "/CN=Kaspa Pulse OpQual Synthetic CA",
            "-addext",
            "basicConstraints=critical,CA:TRUE",
            "-addext",
            "keyUsage=critical,keyCertSign,cRLSign",
            "-out",
            ca_crt.to_str().context("ca crt path")?,
        ],
    )?;
    run(
        "openssl",
        [
            "genrsa",
            "-out",
            key.to_str().context("server key path")?,
            "2048",
        ],
    )?;
    run(
        "openssl",
        [
            "req",
            "-new",
            "-key",
            key.to_str().context("server key path")?,
            "-subj",
            "/CN=api.telegram.org",
            "-out",
            csr.to_str().context("csr path")?,
        ],
    )?;
    fs::write(
        &ext,
        "basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:api.telegram.org,DNS:api.kaspa.org,DNS:api.coingecko.com\n",
    )?;
    run(
        "openssl",
        [
            "x509",
            "-req",
            "-sha256",
            "-days",
            "2",
            "-in",
            csr.to_str().context("csr path")?,
            "-CA",
            ca_crt.to_str().context("ca path")?,
            "-CAkey",
            ca_key.to_str().context("ca key path")?,
            "-CAcreateserial",
            "-extfile",
            ext.to_str().context("ext path")?,
            "-out",
            crt.to_str().context("crt path")?,
        ],
    )?;
    Ok(())
}
pub(super) fn start_fixture(
    ctx: &ContextState,
    name: &str,
    purpose: &str,
    mode: &str,
    network_args: &[&str],
) -> Result<()> {
    if container_exists(name)? {
        assert_owned_container(name)?;
        if docker_inspect(name, "{{.State.Running}}")? != "true" {
            run("sudo", ["-n", "docker", "start", name])?;
        }
        return Ok(());
    }
    let fixture = ctx.fixture_binary.to_str().context("fixture path")?;
    let mut a = vec![
        "-n",
        "docker",
        "run",
        "-d",
        "--name",
        name,
        "--network",
        DOCKER_NETWORK,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    a.extend(network_args.iter().map(|x| (*x).to_owned()));
    a.extend(
        [
            "--label",
            &format!("task={TASK_ID}"),
            "--label",
            &format!("purpose={purpose}"),
            "--read-only",
            "--security-opt",
            "no-new-privileges:true",
            "--tmpfs",
            "/tmp:rw,noexec,nosuid,size=8m",
            "--mount",
            &format!("type=bind,src={fixture},dst=/task/opqual-fixture,readonly"),
            "--mount",
            &format!("type=bind,src={},dst=/control", ctx.control_dir.display()),
            "--mount",
            &format!("type=bind,src={},dst=/evidence", ctx.events_dir.display()),
            "--mount",
            &format!(
                "type=bind,src={},dst=/certs,readonly",
                ctx.certs_dir.display()
            ),
            "--entrypoint",
            "/task/opqual-fixture",
            POSTGRES_IMAGE,
            "--mode",
            mode,
        ]
        .into_iter()
        .map(str::to_owned),
    );
    run("sudo", a)?;
    Ok(())
}
pub(super) fn probe(ctx: &ContextState, url: &str, tls: bool) -> Result<String> {
    let fixture = ctx.fixture_binary.to_str().context("fixture path")?;
    let mut a = vec![
        "-n",
        "docker",
        "run",
        "--rm",
        "--network",
        DOCKER_NETWORK,
        "--mount",
        &format!("type=bind,src={fixture},dst=/task/opqual-fixture,readonly"),
        "--entrypoint",
        "/task/opqual-fixture",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<Vec<_>>();
    if tls {
        a.extend(
            [
                "--mount",
                &format!(
                    "type=bind,src={},dst=/ca.crt,readonly",
                    ctx.certs_dir.join("ca.crt").display()
                ),
            ]
            .into_iter()
            .map(str::to_owned),
        );
    }
    a.extend(
        [POSTGRES_IMAGE, "--mode", "probe", "--url", url]
            .into_iter()
            .map(str::to_owned),
    );
    if tls {
        a.extend(["--ca", "/ca.crt"].into_iter().map(str::to_owned));
    }
    let out = run_capture("sudo", a)?;
    ensure!(
        out.starts_with("STATUS=200\n"),
        "probe failed for {url}: {out}"
    );
    Ok(out)
}

pub(super) fn start_app(ctx: &ContextState) -> Result<()> {
    ctx.require_phase("fixtures")?;
    if maybe_skip_verified(ctx, "start_app")? {
        return Ok(());
    }
    ctx.record(
        "START_APP",
        "PLANNED",
        "launch exact candidate in task-owned internal container",
    )?;
    if container_exists(APP_CONTAINER)? {
        assert_owned_container(APP_CONTAINER)?;
        ensure!(
            docker_inspect(APP_CONTAINER, "{{.State.Running}}")? == "true",
            "existing owned app container is stopped; resume requires reconciliation"
        );
    } else {
        let binary = ctx.binary.to_str().context("candidate path")?;
        let mut a = vec![
            "-n",
            "docker",
            "run",
            "-d",
            "--name",
            APP_CONTAINER,
            "--network",
            DOCKER_NETWORK,
            "--network-alias",
            APP_CONTAINER,
            "--label",
            &format!("task={TASK_ID}"),
            "--label",
            "purpose=exact-candidate",
            "--read-only",
            "--security-opt",
            "no-new-privileges:true",
            "--tmpfs",
            "/tmp:rw,noexec,nosuid,size=16m",
            "--mount",
            &format!("type=bind,src={binary},dst=/task/kaspa-pulse,readonly"),
            "--mount",
            &format!(
                "type=bind,src={},dst=/task/certs,readonly",
                ctx.certs_dir.display()
            ),
            "--mount",
            &format!("type=bind,src={},dst=/evidence", ctx.run_dir.display()),
        ]
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
        for (k, v) in app_env() {
            a.push("-e".to_owned());
            a.push(format!("{k}={v}"));
        }
        a.extend(
            ["--entrypoint", "/task/kaspa-pulse", POSTGRES_IMAGE]
                .into_iter()
                .map(str::to_owned),
        );
        run("sudo", a)?;
    }
    assert_no_published_ports(APP_CONTAINER)?;
    wait_until(
        || {
            Ok(probe(
                ctx,
                &format!("http://{APP_CONTAINER}:{HEALTH_PORT}/readyz"),
                false,
            )
            .map(|x| x.contains("ready"))
            .unwrap_or(false))
        },
        60,
        Duration::from_secs(1),
        "application readiness failed",
    )?;
    let sessions = pg_admin(
        ctx,
        &format!(
            "SELECT count(*) FROM pg_stat_activity WHERE application_name='{APPLICATION_NAME}';"
        ),
    )?;
    ensure!(
        sessions.trim().parse::<u64>().unwrap_or(0) > 0,
        "no PostgreSQL session for application_name={APPLICATION_NAME}"
    );
    let id = docker_inspect(APP_CONTAINER, "{{.Id}}")?;
    let pid = docker_inspect(APP_CONTAINER, "{{.State.Pid}}")?;
    ensure!(
        pid.parse::<u64>().unwrap_or(0) > 1,
        "invalid application host PID"
    );
    ctx.state_set("app_container_id", id.clone())?;
    ctx.state_set("app_host_pid", pid.clone())?;
    atomic_json(
        &ctx.run_dir.join("app-start-verification.json"),
        &json!({
            "application_name":APPLICATION_NAME,"container":APP_CONTAINER,
            "container_id":id,"host_pid":pid.parse::<u64>().unwrap_or(0),
            "health_port":HEALTH_PORT,"binary_sha256":ctx.binary_sha256
        }),
    )?;
    save_app_logs(ctx)?;
    ctx.mark_phase("start_app", "VERIFIED")?;
    ctx.record(
        "START_APP",
        "VERIFIED",
        "exact candidate ready; application_name visible in PostgreSQL",
    )?;
    Ok(())
}

fn app_env() -> Vec<(&'static str, String)> {
    vec![
        ("APP_ENV","development".into()),("RUST_LOG","info".into()),("ENABLE_VERBOSE_LOGS","true".into()),
        ("DATABASE_URL",format!("postgresql://{RUNTIME_ROLE}@{POSTGRES_CONTAINER}:5432/{DATABASE}?sslmode=disable&application_name={APPLICATION_NAME}")),
        ("DB_MAX_CONNECTIONS","8".into()),("ALLOW_RUNTIME_SCHEMA_ENSURE","false".into()),
        ("BOT_TOKEN","1234567890:TEST_TOKEN_NOT_REAL".into()),("ADMIN_ID",SYNTH_ADMIN_ID.to_string()),
        ("ADMIN_USER_ID",SYNTH_ADMIN_ID.to_string()),("ADMIN_CHAT_ID",SYNTH_ADMIN_ID.to_string()),
        ("USE_WEBHOOK","false".into()),("NODE_URL_01",format!("ws://{NODE_CONTAINER}:{NODE_PORT}")),
        ("KASPA_MONITOR_MODE","polling_only".into()),("KASPA_MONITOR_POLL_INTERVAL_SECS","1".into()),
        ("KASPA_MONITOR_RECONCILIATION_INTERVAL_SECS","2".into()),("KASPA_SUBSCRIPTION_MIN_SCAN_INTERVAL_SECS","1".into()),
        ("READINESS_REQUIRE_NODE","true".into()),("READINESS_REQUIRE_SUBSCRIPTION","false".into()),
        ("READINESS_MAX_SCAN_AGE_SECS","120".into()),("HEALTH_ENDPOINT_ENABLED","true".into()),        ("HEALTH_BIND","0.0.0.0".into()),("HEALTH_PORT",HEALTH_PORT.to_string()),
        ("HEALTH_ALLOW_PUBLIC_BIND","true".into()),("ENABLE_TELEGRAM_DELIVERY_QUEUE","true".into()),
        ("TELEGRAM_DELIVERY_MAX_ATTEMPTS","5".into()),("RATE_LIMIT_COMMANDS_PER_SECOND","1000".into()),
        ("RATE_LIMIT_CALLBACKS_PER_SECOND","1000".into()),("RATE_LIMIT_ADD_WALLET_PER_MINUTE","1000".into()),
        ("MAX_WALLETS_PER_USER","50".into()),
        ("COINGECKO_API_URL","https://api.coingecko.com/api/v3/simple/price?ids=kaspa&vs_currencies=usd&include_market_cap=true".into()),
        ("COINGECKO_MARKET_CHART_RANGE_URL","https://api.coingecko.com/api/v3/coins/kaspa/market_chart/range".into()),
        ("KAS_PRICE_HISTORY_ENABLED","true".into()),("KAS_PRICE_REFRESH_INTERVAL_SECS","2".into()),
        ("RPC_TIMEOUT_SECS","5".into()),("HTTP_TIMEOUT_SECS","5".into()),("HTTP_CONNECT_TIMEOUT_SECS","2".into()),
        ("SHUTDOWN_DRAIN_SECS","3".into()),("SSL_CERT_FILE","/task/certs/ca.crt".into()),
        ("PANIC_EVENT_MARKER_PATH","/evidence/panic_event_pending.json".into()),
    ]
}

fn save_app_logs(ctx: &ContextState) -> Result<()> {
    let out = run_capture("sudo", ["-n", "docker", "logs", APP_CONTAINER])?;
    fs::write(ctx.run_dir.join("app.stdout.log"), out)?;
    let err = run_capture("sudo", ["-n", "docker", "logs", "--details", APP_CONTAINER])
        .unwrap_or_default();
    fs::write(ctx.run_dir.join("app.stderr.log"), err)?;
    Ok(())
}

pub(super) fn create_lock(ctx: &ContextState) -> Result<()> {
    ctx.require_phase("start_app")?;
    if maybe_skip_verified(ctx, "create_lock")? {
        return Ok(());
    }
    ctx.record(
        "CREATE_LOCK",
        "PLANNED",
        "hold conflicting advisory lock and prove real app waiter",
    )?;
    assert_owned_container(APP_CONTAINER)?;
    ensure!(
        docker_inspect(APP_CONTAINER, "{{.State.Running}}")? == "true",
        "application not running"
    );
    let token = wallet_token(SYNTH_WALLET);
    let wallet_id = format!("v2:{token}");
    fs::write(
        ctx.run_dir.join("synthetic-lock-identity.txt"),
        format!("chat_id={SYNTH_CHAT_ID}\nwallet={SYNTH_WALLET}\nwallet_identity={wallet_id}\n"),
    )?;
    if container_exists(LOCK_CONTAINER)? {
        assert_owned_container(LOCK_CONTAINER)?;
        ensure!(
            docker_inspect(LOCK_CONTAINER, "{{.State.Running}}")? == "true",
            "stale stopped lock-holder requires reconciliation"
        );
    } else {
        run(
            "sudo",
            [
                "-n",
                "docker",
                "run",
                "-d",
                "--name",
                LOCK_CONTAINER,
                "--network",
                DOCKER_NETWORK,
                "--label",
                &format!("task={TASK_ID}"),
                "--label",
                "purpose=external-lock-holder",
                "-e",
                &format!("PGAPPNAME={LOCK_APP_NAME}"),
                "--entrypoint",
                "psql",
                POSTGRES_IMAGE,
                "-h",
                POSTGRES_CONTAINER,
                "-U",
                OBSERVER_ROLE,
                "-d",
                DATABASE,
                "-X",
                "-v",
                "ON_ERROR_STOP=1",
                "-c",
                &format!(
                    "SELECT pg_backend_pid(); SELECT pg_advisory_lock({SYNTH_CHAT_ID}); SELECT pg_sleep(3600);"
                ),
            ],
        )?;
    }
    assert_no_published_ports(LOCK_CONTAINER)?;
    wait_until(
        || {
            Ok(pg_admin(ctx,&format!(
        "SELECT count(*) FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid WHERE l.locktype='advisory' AND l.granted AND a.application_name='{LOCK_APP_NAME}';"
    ))?.trim().parse::<u64>().unwrap_or(0)>0)
        },
        80,
        Duration::from_millis(250),
        "external advisory lock not acquired",
    )?;
    let existing = pg_admin(
        ctx,
        "SELECT count(*) FROM telegram_delivery_queue WHERE event_key='opqual-v1:blocked-lock';",
    )?;
    match existing.trim() {
        "0" => pg_admin_input(
            ctx,
            &format!(
                "INSERT INTO telegram_delivery_queue(chat_id,message_html,status,wallet_masked,event_key,next_attempt_at) VALUES ({SYNTH_CHAT_ID},'<b>OPQUAL synthetic lock proof</b>','pending','{}','opqual-v1:blocked-lock',NOW());",
                sql_literal(&wallet_id)
            ),
        )?,
        "1" => {}
        other => bail!("unexpected duplicate lock-proof queue rows: {other}"),
    }
    wait_until(
        || {
            Ok(pg_admin(ctx,&format!(r#"
WITH obs AS (
 SELECT l.database,l.classid,l.objid,l.objsubid FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid
 WHERE l.locktype='advisory' AND l.granted AND a.application_name='{LOCK_APP_NAME}'
), app AS (
 SELECT l.database,l.classid,l.objid,l.objsubid,a.wait_event_type,a.query FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid
 WHERE l.locktype='advisory' AND NOT l.granted AND a.application_name='{APPLICATION_NAME}'
)
SELECT count(*) FROM obs JOIN app USING(database,classid,objid,objsubid)
WHERE app.wait_event_type='Lock' AND app.query LIKE '%pg_advisory_xact_lock%';"#))?
        .trim().parse::<u64>().unwrap_or(0)>0)
        },
        80,
        Duration::from_millis(250),
        "real application backend did not enter advisory-lock wait",
    )?;
    let proof = pg_admin(
        ctx,
        &format!(
            "SELECT a.pid,a.application_name,a.state,coalesce(a.wait_event_type,''),coalesce(a.wait_event,''),l.granted,l.classid,l.objid,l.objsubid FROM pg_stat_activity a JOIN pg_locks l ON l.pid=a.pid WHERE l.locktype='advisory' AND a.application_name IN ('{APPLICATION_NAME}','{LOCK_APP_NAME}') ORDER BY a.application_name,l.granted DESC;"
        ),
    )?;
    fs::write(ctx.run_dir.join("pre-sigterm-lock-proof.txt"), &proof)?;
    ensure!(
        proof.contains(APPLICATION_NAME) && proof.contains(LOCK_APP_NAME),
        "lock proof identities missing"
    );
    ctx.mark_phase("create_lock", "VERIFIED")?;
    ctx.state_set("exec01.lock_wait_proven", "true")?;
    ctx.record("CREATE_LOCK","VERIFIED","real delivery worker waits on application advisory lock while external lock remains granted")?;
    Ok(())
}

fn wallet_token(wallet: &str) -> String {
    let mut h = Sha256::new();
    h.update(format!("delivery-wallet-v1:{wallet}").as_bytes());
    h.finalize()
        .iter()
        .take(16)
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub(super) fn exec01(ctx: &ContextState) -> Result<()> {
    ctx.require_phase("create_lock")?;
    ctx.record(
        "EXEC01",
        "PLANNED",
        "SIGTERM exact owned candidate; prove no new work and clean DB/task drain",
    )?;
    let state = ctx.state()?;
    let expected_id = state
        .get("app_container_id")
        .and_then(Value::as_str)
        .context("missing app container id")?;
    let expected_pid = state
        .get("app_host_pid")
        .and_then(Value::as_str)
        .context("missing app host pid")?;
    assert_owned_container(APP_CONTAINER)?;
    ensure!(
        docker_inspect(APP_CONTAINER, "{{.Id}}")? == expected_id,
        "app container identity mismatch"
    );
    if ctx.state_is("exec01.sigterm_sent", "true")? {
        ctx.record(
            "EXEC01_SIGNAL",
            "VERIFIED",
            "SIGTERM already recorded; verifying prior effect without resending",
        )?;
        return verify_shutdown(ctx);
    }
    let running = docker_inspect(APP_CONTAINER, "{{.State.Running}}")?;
    let pid = docker_inspect(APP_CONTAINER, "{{.State.Pid}}")?;
    if running != "true" || pid != expected_pid {
        ctx.state_set("exec01.result", "UNKNOWN")?;
        ctx.record(
            "EXEC01",
            "UNKNOWN",
            "app not running or PID changed before SIGTERM receipt; refusing replay",
        )?;
        bail!("cannot establish signal was never sent; reconcile before retry");
    }
    let utc = Utc::now().to_rfc3339();
    ctx.record(
        "EXEC01_SIGNAL",
        "PLANNED",
        &format!("signal=SIGTERM container={APP_CONTAINER} host_pid={pid}"),
    )?;
    append_jsonl(
        &ctx.run_dir.join("signal-timeline.jsonl"),
        &json!({"utc":utc,"signal":"SIGTERM","container":APP_CONTAINER,"host_pid":pid,"phase":"PLANNED"}),
    )?;
    run(
        "sudo",
        ["-n", "docker", "kill", "--signal=TERM", APP_CONTAINER],
    )?;
    ctx.state_set("exec01.sigterm_sent", "true")?;
    ctx.state_set("exec01.sigterm_utc", utc.clone())?;
    ctx.record(
        "EXEC01_SIGNAL",
        "VERIFIED",
        "SIGTERM accepted for exact task-owned container",
    )?;
    append_jsonl(
        &ctx.run_dir.join("signal-timeline.jsonl"),
        &json!({"utc":utc,"signal":"SIGTERM","container":APP_CONTAINER,"host_pid":pid,"phase":"VERIFIED"}),
    )?;
    let post = pg_admin(
        ctx,
        "SELECT count(*) FROM telegram_delivery_queue WHERE event_key='opqual-v1:post-sigterm';",
    )?;
    match post.trim() {
        "0" => {
            let identity = fs::read_to_string(ctx.run_dir.join("synthetic-lock-identity.txt"))?;
            let wid = identity
                .lines()
                .find_map(|x| x.strip_prefix("wallet_identity="))
                .context("wallet identity missing")?;
            pg_admin_input(
                ctx,
                &format!(
                    "INSERT INTO telegram_delivery_queue(chat_id,message_html,status,wallet_masked,event_key,next_attempt_at) VALUES ({SYNTH_CHAT_ID},'<b>OPQUAL post-SIGTERM admission proof</b>','pending','{}','opqual-v1:post-sigterm',NOW());",
                    sql_literal(wid)
                ),
            )?;
        }
        "1" => {}
        other => {
            ctx.state_set("exec01.result", "FAIL")?;
            bail!("unexpected duplicate post-SIGTERM rows: {other}");
        }
    }
    verify_shutdown(ctx)?;
    ctx.mark_phase("exec01_shutdown", "VERIFIED")?;
    ctx.record(
        "EXEC01",
        "VERIFIED",
        "shutdown half passed; clean restart remains required",
    )?;
    Ok(())
}

fn verify_shutdown(ctx: &ContextState) -> Result<()> {
    ctx.require_phase("create_lock")?;
    ensure!(
        ctx.state_is("exec01.sigterm_sent", "true")?,
        "SIGTERM has not been recorded"
    );
    ctx.record(
        "VERIFY_SHUTDOWN",
        "PLANNED",
        "observe exit, no-new-work and zero PostgreSQL sessions/locks",
    )?;
    let expected = ctx
        .state()?
        .get("app_container_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    ensure!(
        docker_inspect(APP_CONTAINER, "{{.Id}}")? == expected,
        "app container identity changed"
    );
    let activity = ctx.run_dir.join("postgres-activity.jsonl");
    let mut exited = false;
    for _ in 0..120 {
        let running = docker_inspect(APP_CONTAINER, "{{.State.Running}}")? == "true";
        let c = shutdown_counts(ctx)?;
        append_jsonl(
            &activity,
            &json!({"utc":Utc::now().to_rfc3339(),"app_running":running,
            "application_sessions":c.0,"application_locks":c.1,"external_advisory_locks":c.2,"post_sigterm_status":c.3}),
        )?;
        if !running {
            exited = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    ensure!(
        exited,
        "application did not exit within 30s; do not SIGKILL for success"
    );
    let mut final_counts = None;
    for _ in 0..80 {
        let c = shutdown_counts(ctx)?;
        append_jsonl(
            &activity,
            &json!({
                "utc":Utc::now().to_rfc3339(),"app_running":false,
                "application_sessions":c.0,"application_locks":c.1,
                "external_advisory_locks":c.2,"post_sigterm_status":c.3,
                "phase":"post_exit_db_convergence"
            }),
        )?;
        ensure!(c.2 > 0, "external conflicting lock was released too early");
        ensure!(
            c.3 == "pending:",
            "new work was admitted after SIGTERM: {}",
            c.3
        );
        if c.0 == 0 && c.1 == 0 {
            final_counts = Some(c);
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let c = final_counts
        .context("PostgreSQL app backend/locks did not converge to zero after process exit")?;
    let exit_code = docker_inspect(APP_CONTAINER, "{{.State.ExitCode}}")?;
    let oom = docker_inspect(APP_CONTAINER, "{{.State.OOMKilled}}")?;
    ensure!(exit_code == "0", "application exit code={exit_code}");
    ensure!(oom == "false", "application OOM-killed");
    save_app_logs(ctx)?;
    let logs = format!(
        "{}\n{}",
        fs::read_to_string(ctx.run_dir.join("app.stdout.log")).unwrap_or_default(),
        fs::read_to_string(ctx.run_dir.join("app.stderr.log")).unwrap_or_default()
    );
    for marker in [
        "Starting graceful shutdown",
        "All owned background workers and request tasks joined",
        "Database connections closed safely",
    ] {
        ensure!(logs.contains(marker), "shutdown marker missing: {marker}");
    }
    ensure!(
        [
            "shutdown requested",
            "Cancellation requested",
            "Shutdown worker joins complete"
        ]
        .iter()
        .any(|m| logs.contains(m)),
        "cancellation/worker-drain marker missing"
    );
    let finished = docker_inspect(APP_CONTAINER, "{{.State.FinishedAt}}")?;
    ctx.state_set("exec01.exit_code", exit_code)?;
    ctx.state_set("exec01.process_finished_at", finished)?;
    ctx.state_set("exec01.zero_sessions", "true")?;
    ctx.state_set("exec01.zero_locks", "true")?;
    ctx.state_set("exec01.clean_exit", "true")?;
    ctx.state_set("exec01.no_new_work", "true")?;
    ctx.state_set("exec01.shutdown_pass", "true")?;
    ctx.state_set("exec01.forced_kill_used", "false")?;
    atomic_json(
        &ctx.run_dir.join("shutdown-verification.json"),
        &json!({
            "application_long_sql_lock_shutdown":"PASS",
            "zero_app_db_sessions_after_shutdown":"PASS",
            "zero_app_locks_after_shutdown":"PASS",
            "no_new_work_after_shutdown":"PASS",
            "clean_exit":"PASS",
            "forced_kill_used":"NO",
            "exit_code":0,
            "external_lock_still_held":c.2>0
        }),
    )?;
    ctx.mark_phase("verify_shutdown", "VERIFIED")?;
    ctx.record(
        "VERIFY_SHUTDOWN",
        "VERIFIED",
        "SIGTERM shutdown closed app sessions/locks cleanly while external lock remained held",
    )?;
    Ok(())
}

fn shutdown_counts(ctx: &ContextState) -> Result<(u64, u64, u64, String)> {
    let raw = pg_admin(
        ctx,
        &format!(
            r#"
SELECT
 (SELECT count(*) FROM pg_stat_activity WHERE application_name='{APPLICATION_NAME}'),
 (SELECT count(*) FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid WHERE a.application_name='{APPLICATION_NAME}'),
 (SELECT count(*) FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid WHERE a.application_name='{LOCK_APP_NAME}' AND l.locktype='advisory' AND l.granted),
 COALESCE((SELECT status||':'||COALESCE(locked_by,'') FROM telegram_delivery_queue WHERE event_key='opqual-v1:post-sigterm'),'missing');
"#
        ),
    )?;
    let mut parts = raw.trim().split('|');
    let sessions = parts.next().context("missing app session count")?.parse()?;
    let locks = parts.next().context("missing app lock count")?.parse()?;
    let ext = parts
        .next()
        .context("missing external lock count")?
        .parse()?;
    let post = parts.next().unwrap_or("missing").to_owned();
    Ok((sessions, locks, ext, post))
}

pub(super) fn restart(ctx: &ContextState) -> Result<()> {
    ctx.require_phase("verify_shutdown")?;
    if ctx.state_is("phase.restart", "VERIFIED")? {
        ensure!(
            container_exists(APP_CONTAINER)?
                && docker_inspect(APP_CONTAINER, "{{.State.Running}}")? == "true"
                && ctx.state_is("exec01.clean_restart", "true")?,
            "restart receipt exists but current side effects disagree; reconcile before replay"
        );
        ctx.record(
            "RESUME:restart",
            "VERIFIED",
            "restart receipt and current side effects agree; skipped replay",
        )?;
        return Ok(());
    }
    ctx.record(
        "RESTART",
        "PLANNED",
        "release external test lock only after zero proof; restart exact same candidate and perform readiness/DB/Telegram smoke",
    )?;
    assert_owned_container(APP_CONTAINER)?;
    ensure!(
        docker_inspect(APP_CONTAINER, "{{.State.Running}}")? == "false",
        "application still running before restart"
    );
    let lock_pid = pg_admin(
        ctx,
        &format!(
            "SELECT pid FROM pg_stat_activity WHERE application_name='{LOCK_APP_NAME}' ORDER BY pid LIMIT 1;"
        ),
    )?;
    let lock_pid = lock_pid.trim();
    ensure!(
        !lock_pid.is_empty(),
        "external lock backend missing before controlled release"
    );
    let _ = pg_admin(ctx, &format!("SELECT pg_terminate_backend({lock_pid});"))?;
    wait_until(
        || {
            Ok(pg_admin(
                ctx,
                &format!(
                    "SELECT count(*) FROM pg_stat_activity WHERE application_name='{LOCK_APP_NAME}';"
                ),
            )?
            .trim()
            == "0")
        },
        40,
        Duration::from_millis(250),
        "external lock session did not release",
    )?;
    if container_exists(LOCK_CONTAINER)? {
        assert_owned_container(LOCK_CONTAINER)?;
        wait_until(
            || Ok(docker_inspect(LOCK_CONTAINER, "{{.State.Running}}")? == "false"),
            40,
            Duration::from_millis(250),
            "lock-holder container did not stop after backend termination",
        )?;
        run("sudo", ["-n", "docker", "rm", LOCK_CONTAINER])?;
    }
    pg_admin_input(
        ctx,
        "DELETE FROM telegram_delivery_queue WHERE event_key IN ('opqual-v1:blocked-lock','opqual-v1:post-sigterm');",
    )?;
    let event_file = ctx.events_dir.join("telegram-events.jsonl");
    let pre_events = jsonl_count(&event_file)?;
    let started = std::time::Instant::now();
    run("sudo", ["-n", "docker", "start", APP_CONTAINER])?;
    let restart_pid = docker_inspect(APP_CONTAINER, "{{.State.Pid}}")?;
    ensure!(
        restart_pid.parse::<u64>().unwrap_or(0) > 1,
        "restart PID invalid"
    );
    ctx.state_set("exec01.restart_host_pid", restart_pid.clone())?;
    wait_until(
        || {
            Ok(probe(
                ctx,
                &format!("http://{APP_CONTAINER}:{HEALTH_PORT}/readyz"),
                false,
            )
            .map(|x| x.contains("ready"))
            .unwrap_or(false))
        },
        60,
        Duration::from_secs(1),
        "clean restart readiness failed",
    )?;
    let health = probe(
        ctx,
        &format!("http://{APP_CONTAINER}:{HEALTH_PORT}/healthz"),
        false,
    )?;
    let ready = probe(
        ctx,
        &format!("http://{APP_CONTAINER}:{HEALTH_PORT}/readyz"),
        false,
    )?;
    ensure!(health.contains("ok"), "restart health failed: {health}");
    ensure!(ready.contains("ready"), "restart readiness failed: {ready}");
    let sessions = pg_admin(
        ctx,
        &format!(
            "SELECT count(*) FROM pg_stat_activity WHERE application_name='{APPLICATION_NAME}';"
        ),
    )?;
    ensure!(
        sessions.trim().parse::<u64>().unwrap_or(0) > 0,
        "restart DB connectivity/application_name missing"
    );
    inject_message(ctx, SYNTH_USER_ID, SYNTH_CHAT_ID, "/help")?;
    wait_telegram_text(ctx, pre_events, "Kaspa Pulse Help", Duration::from_secs(15))?;
    let restart_ms = started.elapsed().as_millis() as u64;
    ctx.state_set("exec01.restart_duration_ms", restart_ms)?;
    ctx.state_set("exec01.clean_restart", "true")?;
    ctx.state_set("exec01.result", "PASS")?;
    atomic_json(
        &ctx.run_dir.join("restart-verification.json"),
        &json!({
            "clean_restart":"PASS",
            "health":"PASS",
            "readiness":"PASS",
            "database_connectivity":"PASS",
            "functional_smoke":"PASS",
            "application_name":APPLICATION_NAME,
            "restart_host_pid":restart_pid.parse::<u64>().unwrap_or(0),
            "restart_duration_ms":restart_ms,
            "same_candidate_sha256":ctx.binary_sha256
        }),
    )?;
    ctx.mark_phase("restart", "VERIFIED")?;
    ctx.mark_phase("exec01", "VERIFIED")?;
    ctx.record(
        "RESTART",
        "VERIFIED",
        "same exact candidate restarted; health/readiness/DB/Telegram smoke pass",
    )?;
    ctx.record(
        "EXEC01",
        "VERIFIED",
        "long-lock SIGTERM shutdown plus zero sessions/locks, clean exit, no SIGKILL, clean restart all pass",
    )?;
    Ok(())
}

fn jsonl_count(path: &Path) -> Result<usize> {
    if !path.exists() {
        return Ok(0);
    }
    Ok(fs::read_to_string(path)?
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count())
}

fn inject_message(ctx: &ContextState, user_id: i64, chat_id: i64, text: &str) -> Result<u64> {
    let path = ctx.control_dir.join("telegram-updates.jsonl");
    let mut max_id = 10_000u64;
    if path.exists() {
        for line in fs::read_to_string(&path)?.lines() {
            if let Ok(row) = serde_json::from_str::<Value>(line) {
                max_id = max_id.max(row.get("update_id").and_then(Value::as_u64).unwrap_or(0));
            }
        }
    }
    let update_id = max_id + 1;
    append_jsonl(
        &path,
        &json!({
            "update_id":update_id,
            "message":{
                "message_id":update_id,
                "date":Utc::now().timestamp(),
                "chat":{"id":chat_id,"type":"private","first_name":"OpQualUser"},
                "from":{"id":user_id,"is_bot":false,"first_name":"OpQualUser","username":"opqual_user"},
                "text":text
            }
        }),
    )?;
    Ok(update_id)
}

fn wait_telegram_text(
    ctx: &ContextState,
    start: usize,
    needle: &str,
    timeout: Duration,
) -> Result<()> {
    let path = ctx.events_dir.join("telegram-events.jsonl");
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if path.exists() {
            let rows = fs::read_to_string(&path)?;
            for line in rows.lines().skip(start) {
                if let Ok(row) = serde_json::from_str::<Value>(line) {
                    let method = row
                        .get("method")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if [
                        "sendmessage",
                        "editmessagetext",
                        "editmessagereplymarkup",
                        "answercallbackquery",
                    ]
                    .contains(&method.as_str())
                        && row.to_string().contains(needle)
                    {
                        return Ok(());
                    }
                }
            }
        }
        ensure!(
            std::time::Instant::now() < deadline,
            "timeout waiting for Telegram event containing {needle:?}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}
