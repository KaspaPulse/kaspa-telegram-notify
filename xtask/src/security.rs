use anyhow::{Context, Result, bail, ensure};
use chrono::{Duration as ChronoDuration, NaiveDate, Utc};
use regex::Regex;
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;
use toml::Value as TomlValue;

const KASPA_GIT_URL: &str = "https://github.com/kaspanet/rusty-kaspa";
const KASPA_DIRECT_PACKAGES: [&str; 5] = [
    "kaspa-wrpc-client",
    "kaspa-rpc-core",
    "kaspa-addresses",
    "kaspa-consensus-core",
    "kaspa-hashes",
];

fn read_text(root: &Path, relative: &str) -> Result<String> {
    fs::read_to_string(root.join(relative)).with_context(|| format!("failed to read {relative}"))
}

fn proof_policy(root: &Path) -> Result<TomlValue> {
    let policy: TomlValue = read_text(root, "proof/policy.toml")?
        .parse()
        .context("failed to parse proof/policy.toml")?;
    ensure!(
        policy.get("schema_version").and_then(TomlValue::as_str)
            == Some(crate::proof::PROOF_SCHEMA_VERSION),
        "unsupported proof policy schema version"
    );
    ensure!(
        policy.get("policy_version").and_then(TomlValue::as_integer) == Some(4),
        "unsupported proof policy version"
    );
    Ok(policy)
}

fn required_policy_str<'a>(
    table: &'a toml::map::Map<String, TomlValue>,
    key: &str,
) -> Result<&'a str> {
    table
        .get(key)
        .and_then(TomlValue::as_str)
        .with_context(|| format!("proof policy missing string field {key}"))
}

fn require_policy_value(
    table: &toml::map::Map<String, TomlValue>,
    key: &str,
    expected: &str,
) -> Result<()> {
    let actual = required_policy_str(table, key)?;
    ensure!(
        actual == expected,
        "proof policy {key} must be {expected}, got {actual}"
    );
    Ok(())
}

pub fn environment_boundary(root: impl AsRef<Path>) -> Result<()> {
    let root = root.as_ref();
    let policy = proof_policy(root)?;
    let boundary = policy
        .get("environment_boundary")
        .and_then(TomlValue::as_table)
        .context("proof policy missing [environment_boundary]")?;

    for (key, expected) in [
        ("production_source_mutation", "FORBIDDEN"),
        ("production_source_build", "FORBIDDEN"),
        ("production_as_ci_runner", "FORBIDDEN"),
        ("local_build_role", "QUALIFICATION_ONLY"),
        (
            "canonical_release_artifact",
            "PUBLISHED_ATTESTED_GITHUB_RELEASE",
        ),
        ("private_host_identity_in_public_policy", "FORBIDDEN"),
    ] {
        require_policy_value(boundary, key, expected)?;
    }

    let identity = policy
        .get("ci_identity")
        .and_then(TomlValue::as_table)
        .context("proof policy missing [ci_identity]")?;
    for (key, expected) in [
        ("fixture_kind", "SYNTHETIC"),
        ("external_telegram_access", "FORBIDDEN"),
        ("real_user_lookup", "FORBIDDEN"),
        ("real_message_delivery", "FORBIDDEN"),
    ] {
        require_policy_value(identity, key, expected)?;
    }

    let admin_user_id = required_policy_str(identity, "admin_user_id")?
        .parse::<i64>()
        .context("ci_identity.admin_user_id must be a positive i64")?;
    let admin_chat_id = required_policy_str(identity, "admin_chat_id")?
        .parse::<i64>()
        .context("ci_identity.admin_chat_id must be a positive i64")?;
    ensure!(
        admin_user_id > 0 && admin_chat_id == admin_user_id,
        "CI synthetic admin identity must be a positive private-chat identity"
    );

    let api_url = required_policy_str(identity, "telegram_api_url")?;
    let socket = api_url
        .strip_prefix("http://")
        .and_then(|value| value.strip_suffix('/'))
        .context("CI Telegram API URL must use http://<loopback>:<port>/")?
        .parse::<std::net::SocketAddr>()
        .context("CI Telegram API URL must contain a numeric loopback socket address")?;
    ensure!(
        socket.ip().is_loopback() && socket.port() > 0,
        "CI Telegram API URL must be loopback-only"
    );

    for workflow in [
        ".github/workflows/rust-ci.yml",
        ".github/workflows/auto-rusty-kaspa-update.yml",
    ] {
        let text = read_text(root, workflow)?;
        for (key, value) in [
            (
                "ADMIN_USER_ID",
                required_policy_str(identity, "admin_user_id")?,
            ),
            (
                "ADMIN_CHAT_ID",
                required_policy_str(identity, "admin_chat_id")?,
            ),
            ("TELEGRAM_API_URL", api_url),
        ] {
            ensure!(
                text.contains(&format!("{key}: \"{value}\"")),
                "{workflow} {key} diverges from canonical CI identity policy"
            );
        }
        ensure!(
            text.contains("TEST_PLACEHOLDER_TOKEN_NOT_REAL"),
            "{workflow} must use the documented synthetic BOT_TOKEN fixture"
        );
        ensure!(
            !text.contains("ADMIN_ID:"),
            "{workflow} must use explicit synthetic user/chat identity fields"
        );
    }

    println!("environment-boundary-check: PASS schema=1.3.0 policy=4");
    Ok(())
}

pub fn kaspa_pins(
    root: impl AsRef<Path>,
    expected_version: Option<&str>,
    expected_rev: Option<&str>,
) -> Result<()> {
    let root = root.as_ref();
    let cargo: TomlValue = read_text(root, "Cargo.toml")?
        .parse()
        .context("failed to parse Cargo.toml")?;
    let dependencies = cargo
        .get("dependencies")
        .and_then(TomlValue::as_table)
        .context("Cargo.toml is missing [dependencies]")?;

    let exact_version = Regex::new(r"^=([0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?)$")?;
    let full_sha = Regex::new(r"^[0-9a-f]{40}$")?;
    let mut versions = BTreeSet::new();
    let mut revisions = BTreeSet::new();

    for name in KASPA_DIRECT_PACKAGES {
        let spec = dependencies
            .get(name)
            .and_then(TomlValue::as_table)
            .with_context(|| format!("{name} must be an inline/table dependency specification"))?;
        ensure!(
            spec.get("git").and_then(TomlValue::as_str) == Some(KASPA_GIT_URL),
            "{name} must use reviewed Git source {KASPA_GIT_URL}"
        );
        ensure!(
            !spec.contains_key("tag") && !spec.contains_key("branch"),
            "{name} must not use mutable tag/branch references"
        );
        let version = spec
            .get("version")
            .and_then(TomlValue::as_str)
            .with_context(|| format!("{name} must define an exact version"))?;
        let captures = exact_version
            .captures(version)
            .with_context(|| format!("{name} must use an exact '=x.y.z' version requirement"))?;
        let revision = spec
            .get("rev")
            .and_then(TomlValue::as_str)
            .with_context(|| format!("{name} must define rev"))?;
        ensure!(
            full_sha.is_match(revision),
            "{name} must use a full lowercase 40-character rev"
        );
        versions.insert(captures[1].to_owned());
        revisions.insert(revision.to_owned());
    }

    ensure!(
        versions.len() == 1 && revisions.len() == 1,
        "all direct rusty-kaspa dependencies must share one exact version and revision"
    );
    let version = versions.iter().next().expect("one version");
    let revision = revisions.iter().next().expect("one revision");

    if let Some(expected) = expected_version {
        ensure!(
            version == expected,
            "rusty-kaspa version mismatch: expected {expected}, found {version}"
        );
    }
    if let Some(expected) = expected_rev {
        ensure!(
            revision == expected,
            "rusty-kaspa revision mismatch: expected {expected}, found {revision}"
        );
    }

    let lock: TomlValue = read_text(root, "Cargo.lock")?
        .parse()
        .context("failed to parse Cargo.lock")?;
    let packages = lock
        .get("package")
        .and_then(TomlValue::as_array)
        .context("Cargo.lock contains no package array")?;
    let expected_source = format!("git+{KASPA_GIT_URL}?rev={revision}#{revision}");
    let mut locked_count = 0usize;
    for package in packages {
        let Some(table) = package.as_table() else {
            continue;
        };
        let source = table
            .get("source")
            .and_then(TomlValue::as_str)
            .unwrap_or_default();
        if !source.contains(KASPA_GIT_URL) {
            continue;
        }
        locked_count += 1;
        ensure!(
            source == expected_source,
            "Cargo.lock source drift for {}: {source}",
            table
                .get("name")
                .and_then(TomlValue::as_str)
                .unwrap_or("<unknown>")
        );
        ensure!(
            table.get("version").and_then(TomlValue::as_str) == Some(version.as_str()),
            "Cargo.lock version drift for {}",
            table
                .get("name")
                .and_then(TomlValue::as_str)
                .unwrap_or("<unknown>")
        );
    }
    ensure!(
        locked_count > 0,
        "Cargo.lock contains no rusty-kaspa packages"
    );

    let deny: TomlValue = read_text(root, "deny.toml")?
        .parse()
        .context("failed to parse deny.toml")?;
    ensure!(
        deny.get("bans")
            .and_then(TomlValue::as_table)
            .and_then(|table| table.get("wildcards"))
            .and_then(TomlValue::as_str)
            == Some("deny"),
        "deny.toml must enforce bans.wildcards = 'deny'"
    );
    let allow_git = deny
        .get("sources")
        .and_then(TomlValue::as_table)
        .and_then(|table| table.get("allow-git"))
        .and_then(TomlValue::as_array)
        .context("deny.toml must define sources.allow-git")?;
    ensure!(
        allow_git
            .iter()
            .filter_map(TomlValue::as_str)
            .any(|url| url == KASPA_GIT_URL),
        "deny.toml must explicitly allow the reviewed rusty-kaspa source"
    );

    println!(
        "rusty-kaspa-pin-check: PASS version={version} rev={revision} locked_packages={locked_count}"
    );
    Ok(())
}

fn date_from_toml(value: &TomlValue, advisory: &str) -> Result<NaiveDate> {
    let raw = match value {
        TomlValue::String(value) => value.clone(),
        TomlValue::Datetime(value) => value.to_string(),
        _ => bail!("{advisory} must define ignoreUntil as YYYY-MM-DD"),
    };
    let date = raw
        .get(..10)
        .with_context(|| format!("invalid ignoreUntil for {advisory}: {raw}"))?;
    NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .with_context(|| format!("invalid ignoreUntil for {advisory}: {raw}"))
}

pub fn advisories(root: impl AsRef<Path>, max_age_days: i64) -> Result<()> {
    ensure!(max_age_days > 0, "--max-age-days must be positive");
    let root = root.as_ref();
    let policy = proof_policy(root)?;
    let canonical = policy
        .get("advisory_exception")
        .and_then(TomlValue::as_array)
        .context("proof policy must define [[advisory_exception]] records")?;
    let rustsec = Regex::new(r"RUSTSEC-\d{4}-\d{4}")?;
    let today = Utc::now().date_naive();
    let latest_allowed = today + ChronoDuration::days(max_age_days);

    let mut canonical_ids = BTreeSet::new();
    let mut records = BTreeMap::new();
    let mut expected_osv = BTreeSet::new();
    let mut expected_audit = BTreeSet::new();
    let mut expected_deny = BTreeSet::new();

    for (index, entry) in canonical.iter().enumerate() {
        let table = entry
            .as_table()
            .with_context(|| format!("advisory_exception #{} must be a table", index + 1))?;
        let advisory = required_policy_str(table, "id")?;
        ensure!(
            rustsec
                .find(advisory)
                .is_some_and(|found| found.as_str() == advisory),
            "advisory_exception #{} has invalid id {advisory}",
            index + 1
        );
        ensure!(
            canonical_ids.insert(advisory.to_owned()),
            "duplicate canonical advisory exception: {advisory}"
        );

        for field in [
            "package",
            "dependency_path",
            "direct_or_transitive",
            "runtime_or_build_time",
            "target_reachability",
            "security_impact",
            "justification",
            "reviewed_at",
            "expires_at",
            "remediation_trigger",
            "upstream_blocker_if_any",
        ] {
            ensure!(
                !required_policy_str(table, field)?.trim().is_empty(),
                "{advisory} canonical field {field} must not be empty"
            );
        }

        let reviewed =
            NaiveDate::parse_from_str(required_policy_str(table, "reviewed_at")?, "%Y-%m-%d")
                .with_context(|| format!("{advisory} reviewed_at must be YYYY-MM-DD"))?;
        let expiry =
            NaiveDate::parse_from_str(required_policy_str(table, "expires_at")?, "%Y-%m-%d")
                .with_context(|| format!("{advisory} expires_at must be YYYY-MM-DD"))?;
        let age_days = (today - reviewed).num_days();
        ensure!(age_days >= 0, "{advisory} review date is in the future");
        ensure!(
            age_days <= max_age_days,
            "{advisory} review is {age_days} days old; maximum is {max_age_days}"
        );
        ensure!(
            expiry >= today,
            "canonical exception expired for {advisory} on {expiry}"
        );
        ensure!(
            expiry <= latest_allowed,
            "canonical exception for {advisory} expires beyond the {max_age_days}-day window"
        );

        let scanners = table
            .get("scanners")
            .and_then(TomlValue::as_array)
            .context("canonical advisory scanners must be an array")?;
        ensure!(
            !scanners.is_empty(),
            "{advisory} must name at least one scanner"
        );
        for scanner in scanners {
            match scanner.as_str().context("scanner name must be a string")? {
                "OSV" => {
                    expected_osv.insert(advisory.to_owned());
                }
                "CARGO_AUDIT" => {
                    expected_audit.insert(advisory.to_owned());
                }
                "CARGO_DENY" => {
                    expected_deny.insert(advisory.to_owned());
                }
                other => bail!("unknown scanner {other} for {advisory}"),
            }
        }
        records.insert(advisory.to_owned(), table);
    }

    let scanner_ids = |text: &str| -> BTreeSet<String> {
        rustsec
            .find_iter(text)
            .map(|item| item.as_str().to_owned())
            .collect()
    };

    let audit = read_text(root, ".cargo/audit.toml")?;
    let deny = read_text(root, "deny.toml")?;
    ensure!(
        scanner_ids(&audit) == expected_audit,
        "cargo-audit ignore set diverges from canonical advisory policy"
    );
    ensure!(
        scanner_ids(&deny) == expected_deny,
        "cargo-deny ignore set diverges from canonical advisory policy"
    );

    let osv_text = read_text(root, "osv-scanner.toml")?;
    let osv: TomlValue = osv_text.parse().context("cannot parse osv-scanner.toml")?;
    let entries = osv
        .get("IgnoredVulns")
        .and_then(TomlValue::as_array)
        .context("osv-scanner.toml IgnoredVulns must be an array of tables")?;
    let mut actual_osv = BTreeSet::new();
    for entry in entries {
        let table = entry.as_table().context("OSV exception must be a table")?;
        let advisory = required_policy_str(table, "id")?;
        ensure!(
            actual_osv.insert(advisory.to_owned()),
            "duplicate OSV ignored advisory: {advisory}"
        );
        let canonical = records
            .get(advisory)
            .with_context(|| format!("OSV exception {advisory} is not canonical"))?;
        ensure!(
            required_policy_str(table, "reason")?
                == required_policy_str(canonical, "justification")?,
            "OSV reason diverges from canonical justification for {advisory}"
        );
        let expiry = date_from_toml(
            table
                .get("ignoreUntil")
                .with_context(|| format!("{advisory} must define ignoreUntil"))?,
            advisory,
        )?;
        ensure!(
            expiry.format("%Y-%m-%d").to_string() == required_policy_str(canonical, "expires_at")?,
            "OSV expiry diverges from canonical expiry for {advisory}"
        );
    }
    ensure!(
        actual_osv == expected_osv,
        "OSV ignore set diverges from canonical advisory policy"
    );

    let report = read_text(root, "SECURITY_ADVISORIES.md")?;
    ensure!(
        scanner_ids(&report) == canonical_ids,
        "SECURITY_ADVISORIES.md must contain exactly the current canonical advisory IDs"
    );

    println!(
        "security-advisory-check: PASS canonical={} osv={} audit={} deny={}",
        canonical_ids.len(),
        expected_osv.len(),
        expected_audit.len(),
        expected_deny.len()
    );
    Ok(())
}

fn validate_public_topology_text(relative: &str, text: &str) -> Result<()> {
    let named_host =
        Regex::new(r"(?im)^\s*(?:[-*]\s*)?(?:development|production|qualification)\s+host\s*[:=]")?;
    let private_ipv4 = Regex::new(
        r"\b(?:10\.\d{1,3}\.\d{1,3}\.\d{1,3}|192\.168\.\d{1,3}\.\d{1,3}|172\.(?:1[6-9]|2\d|3[01])\.\d{1,3}\.\d{1,3})\b",
    )?;
    let private_home = Regex::new(r"(?:/home/|/Users/)[A-Za-z0-9._-]+/")?;
    ensure!(
        !named_host.is_match(text),
        "{relative} exposes a host-specific operational identity"
    );
    ensure!(
        !private_ipv4.is_match(text),
        "{relative} exposes an RFC1918 address"
    );
    ensure!(
        !private_home.is_match(text),
        "{relative} exposes a private home-directory path"
    );
    Ok(())
}

pub fn documentation(root: impl AsRef<Path>) -> Result<()> {
    let root = root.as_ref();

    for relative in [
        "AGENTS.md",
        "CONTRIBUTING.md",
        "README.md",
        "SECURITY.md",
        "SUPPLY_CHAIN.md",
        "docs/OPERATIONAL_QUALIFICATION.md",
    ] {
        validate_public_topology_text(relative, &read_text(root, relative)?)?;
    }

    let markdown_link = Regex::new(r"\[[^\]]+\]\(([^)]+)\)")?;
    for relative in [
        "README.md",
        "SECURITY.md",
        "SUPPLY_CHAIN.md",
        "CONTRIBUTING.md",
        "AGENTS.md",
    ] {
        let text = read_text(root, relative)?;
        for captures in markdown_link.captures_iter(&text) {
            let raw = captures.get(1).map(|m| m.as_str()).unwrap_or_default();
            if raw.starts_with("http://")
                || raw.starts_with("https://")
                || raw.starts_with("mailto:")
                || raw.starts_with('#')
            {
                continue;
            }
            let path = raw.split('#').next().unwrap_or_default();
            ensure!(
                !path.is_empty() && root.join(path).exists(),
                "{relative} contains dangling local link: {raw}"
            );
        }
    }

    let insights = read_text(root, "security-insights.yml")?;
    let blob_path =
        Regex::new(r#"https://github\.com/KaspaPulse/kaspa-telegram-notify/blob/main/([^"\s]+)"#)?;
    for captures in blob_path.captures_iter(&insights) {
        let path = captures.get(1).map(|m| m.as_str()).unwrap_or_default();
        ensure!(
            root.join(path).exists(),
            "security-insights.yml references missing repository path: {path}"
        );
    }

    let surfaces: JsonValue =
        serde_json::from_str(&read_text(root, "opqual/scenario-surfaces-v1.json")?)
            .context("invalid opqual/scenario-surfaces-v1.json")?;
    let policies = surfaces
        .get("policies")
        .and_then(JsonValue::as_object)
        .context("scenario surfaces missing policies")?;
    for (name, policy) in policies {
        for key in ["critical_exact", "proven_irrelevant_exact"] {
            let values = policy
                .get(key)
                .and_then(JsonValue::as_array)
                .with_context(|| format!("scenario policy {name} missing {key}"))?;
            for value in values {
                let path = value.as_str().with_context(|| {
                    format!("scenario policy {name}.{key} must contain strings")
                })?;
                ensure!(
                    root.join(path).is_file(),
                    "scenario policy {name}.{key} references missing file: {path}"
                );
            }
        }
        for key in ["critical_prefixes", "proven_irrelevant_prefixes"] {
            let values = policy
                .get(key)
                .and_then(JsonValue::as_array)
                .with_context(|| format!("scenario policy {name} missing {key}"))?;
            for value in values {
                let path = value.as_str().with_context(|| {
                    format!("scenario policy {name}.{key} must contain strings")
                })?;
                ensure!(
                    root.join(path.trim_end_matches('/')).is_dir(),
                    "scenario policy {name}.{key} references missing directory: {path}"
                );
            }
        }
    }

    let policy = proof_policy(root)?;
    for key in ["required_ci_workflow", "msrv_ci_workflow"] {
        let path = policy
            .get(key)
            .and_then(TomlValue::as_str)
            .with_context(|| format!("proof policy missing {key}"))?;
        ensure!(
            root.join(path).is_file(),
            "proof policy {key} path is missing: {path}"
        );
    }

    let toolchain: TomlValue = read_text(root, "rust-toolchain.toml")?
        .parse()
        .context("invalid rust-toolchain.toml")?;
    let channel = toolchain
        .get("toolchain")
        .and_then(TomlValue::as_table)
        .and_then(|table| table.get("channel"))
        .and_then(TomlValue::as_str)
        .context("rust-toolchain.toml missing toolchain.channel")?;
    let cargo: TomlValue = read_text(root, "Cargo.toml")?
        .parse()
        .context("invalid Cargo.toml")?;
    let msrv = cargo
        .get("package")
        .and_then(TomlValue::as_table)
        .and_then(|table| table.get("rust-version"))
        .and_then(TomlValue::as_str)
        .context("Cargo.toml missing package.rust-version")?;
    let readme = read_text(root, "README.md")?;
    let badge = Regex::new(r"Rust-([0-9]+\.[0-9]+\.[0-9]+)-orange")?;
    let displayed = badge
        .captures(&readme)
        .and_then(|captures| captures.get(1))
        .map(|m| m.as_str())
        .context("README Rust badge version is missing")?;
    ensure!(
        displayed == channel,
        "README Rust badge {displayed} does not match rust-toolchain.toml {channel}"
    );
    ensure!(
        readme.matches(channel).count() == 1,
        "README must display the primary Rust toolchain version exactly once"
    );
    if msrv != channel {
        ensure!(
            !readme.contains(msrv),
            "README must refer to Cargo.toml for MSRV instead of duplicating the numeric value"
        );
    }
    ensure!(
        !readme.contains("latest verified stable release"),
        "README must not contain a time-sensitive latest-stable dependency claim"
    );

    let supply = read_text(root, "SUPPLY_CHAIN.md")?;
    ensure!(
        supply.contains(&format!(
            "schema is pinned to `{}`",
            crate::proof::PROOF_SCHEMA_VERSION
        )),
        "SUPPLY_CHAIN.md proof schema statement is stale"
    );

    println!(
        "documentation-contract-check: PASS toolchain={channel} schema={}",
        crate::proof::PROOF_SCHEMA_VERSION
    );
    Ok(())
}

const SCORECARD_EXCLUDED_RULE_IDS: [&str; 3] =
    ["BranchProtectionID", "CIIBestPracticesID", "CodeReviewID"];

pub fn filter_scorecard_sarif(document: &mut JsonValue) -> Result<BTreeMap<String, usize>> {
    let runs = document
        .get_mut("runs")
        .and_then(JsonValue::as_array_mut)
        .context("SARIF document must contain a runs array")?;
    let excluded: BTreeSet<&str> = SCORECARD_EXCLUDED_RULE_IDS.into_iter().collect();
    let mut removed = BTreeMap::<String, usize>::new();

    for run in runs {
        let run = run
            .as_object_mut()
            .context("every SARIF run must be an object")?;
        let results = run
            .entry("results")
            .or_insert_with(|| JsonValue::Array(Vec::new()))
            .as_array_mut()
            .context("SARIF run results must be an array")?;
        let original = std::mem::take(results);
        for result in original {
            let object = result
                .as_object()
                .context("every SARIF result must be an object")?;
            let rule_id = object.get("ruleId").and_then(JsonValue::as_str);
            if let Some(rule_id) = rule_id.filter(|rule_id| excluded.contains(*rule_id)) {
                *removed.entry(rule_id.to_owned()).or_default() += 1;
            } else {
                results.push(result);
            }
        }
    }
    Ok(removed)
}

pub fn scorecard_filter(input: &Path, output: &Path) -> Result<()> {
    let mut document: JsonValue = serde_json::from_str(
        &fs::read_to_string(input)
            .with_context(|| format!("failed to read {}", input.display()))?,
    )
    .with_context(|| format!("failed to parse {}", input.display()))?;
    ensure!(document.is_object(), "SARIF root must be an object");
    let removed = filter_scorecard_sarif(&mut document)?;
    let encoded = serde_json::to_string_pretty(&document)? + "\n";
    fs::write(output, encoded).with_context(|| format!("failed to write {}", output.display()))?;
    let summary = SCORECARD_EXCLUDED_RULE_IDS
        .iter()
        .map(|rule| format!("{rule}={}", removed.get(*rule).copied().unwrap_or(0)))
        .collect::<Vec<_>>()
        .join(", ");
    println!("scorecard-sarif-filter: PASS ({summary})");
    Ok(())
}

fn uses_reference(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let value = trimmed
        .strip_prefix("uses:")
        .or_else(|| trimmed.strip_prefix("- uses:"))?
        .trim();
    let value = value.split('#').next().unwrap_or(value).trim();
    Some(value.trim_matches(['\'', '"']))
}

fn is_full_sha_action_reference(reference: &str) -> bool {
    if reference.starts_with("./") || reference.starts_with("docker://") {
        return true;
    }
    let Some((_, revision)) = reference.rsplit_once('@') else {
        return false;
    };
    revision.len() == 40
        && revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub fn action_pins(root: impl AsRef<Path>) -> Result<()> {
    let workflow_dir = root.as_ref().join(".github/workflows");
    let mut files = fs::read_dir(&workflow_dir)
        .with_context(|| format!("failed to read {}", workflow_dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()?;
    files.sort_by_key(|entry| entry.file_name());

    let mut violations = Vec::new();
    let mut checked = 0usize;
    for entry in files {
        let path = entry.path();
        let extension = path.extension().and_then(|ext| ext.to_str());
        if !matches!(extension, Some("yml" | "yaml")) {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        for (index, line) in text.lines().enumerate() {
            let Some(reference) = uses_reference(line) else {
                continue;
            };
            checked += 1;
            if !is_full_sha_action_reference(reference) {
                violations.push(format!(
                    "{}:{} external uses reference is not pinned to a lowercase full 40-character SHA: {}",
                    path.display(),
                    index + 1,
                    reference
                ));
            }
        }
    }

    ensure!(
        violations.is_empty(),
        "GitHub Action pin policy violations:\n{}",
        violations.join("\n")
    );
    println!("action-pin-check: PASS checked_references={checked}");
    Ok(())
}

pub fn non_rust_exec(root: impl AsRef<Path>) -> Result<()> {
    let root = root.as_ref();
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z"])
        .output()
        .context("failed to execute git ls-files")?;
    ensure!(
        output.status.success(),
        "git ls-files failed with status {}",
        output.status
    );
    let paths = String::from_utf8(output.stdout).context("git paths are not UTF-8")?;
    let banned_extensions = [
        "sh", "bash", "py", "ps1", "js", "mjs", "cjs", "ts", "rb", "pl", "php",
    ];
    let mut violations = BTreeSet::new();

    for relative in paths.split('\0').filter(|value| !value.is_empty()) {
        let path = Path::new(relative);
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| banned_extensions.contains(&ext))
        {
            violations.insert(relative.to_owned());
            continue;
        }
        let full = root.join(path);
        if let Ok(bytes) = fs::read(&full)
            && bytes.starts_with(b"#!")
            && path.extension().and_then(|ext| ext.to_str()) != Some("rs")
        {
            violations.insert(relative.to_owned());
        }
    }

    ensure!(
        violations.is_empty(),
        "non-Rust first-party executable source remains:\n{}",
        violations.into_iter().collect::<Vec<_>>().join("\n")
    );
    println!("non-rust-exec-check: PASS");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scorecard_filter_removes_only_approved_governance_rules() {
        let mut document = serde_json::json!({
            "version": "2.1.0",
            "runs": [{
                "results": [
                    {"ruleId": "CodeReviewID"},
                    {"ruleId": "BranchProtectionID"},
                    {"ruleId": "CIIBestPracticesID"},
                    {"ruleId": "VulnerabilitiesID"},
                    {"ruleId": "DangerousWorkflowID"}
                ]
            }]
        });
        let removed = filter_scorecard_sarif(&mut document).unwrap();
        let kept = document["runs"][0]["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value["ruleId"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(kept, ["VulnerabilitiesID", "DangerousWorkflowID"]);
        assert_eq!(removed.get("CodeReviewID"), Some(&1));
        assert!(!removed.contains_key("VulnerabilitiesID"));
    }

    #[test]
    fn scorecard_filter_preserves_unknown_findings_fail_closed() {
        let mut document = serde_json::json!({
            "runs": [{
                "results": [
                    {"ruleId": "FutureSecurityFindingID"},
                    {"ruleId": "CodeReviewID"}
                ]
            }]
        });
        filter_scorecard_sarif(&mut document).unwrap();
        assert_eq!(
            document["runs"][0]["results"],
            serde_json::json!([{"ruleId": "FutureSecurityFindingID"}])
        );
    }

    #[test]
    fn scorecard_filter_fails_closed_on_malformed_input() {
        let mut document = serde_json::json!({});
        assert!(filter_scorecard_sarif(&mut document).is_err());
    }

    #[test]
    fn scorecard_filter_round_trip_writes_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.sarif");
        let target = dir.path().join("target.sarif");
        fs::write(
            &source,
            r#"{"runs":[{"results":[{"ruleId":"CodeReviewID"}]}]}"#,
        )
        .unwrap();
        scorecard_filter(&source, &target).unwrap();
        let document: JsonValue =
            serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(document["runs"][0]["results"], serde_json::json!([]));
    }

    fn canonical_environment_policy() -> String {
        r#"schema_version = "1.3.0"
policy_version = 4
required_ci_workflow = ".github/workflows/security.yml"
msrv_ci_workflow = ".github/workflows/rust-ci.yml"

[environment_boundary]
production_source_mutation = "FORBIDDEN"
production_source_build = "FORBIDDEN"
production_as_ci_runner = "FORBIDDEN"
local_build_role = "QUALIFICATION_ONLY"
canonical_release_artifact = "PUBLISHED_ATTESTED_GITHUB_RELEASE"
private_host_identity_in_public_policy = "FORBIDDEN"

[ci_identity]
fixture_kind = "SYNTHETIC"
admin_user_id = "9000000000000000000"
admin_chat_id = "9000000000000000000"
telegram_api_url = "http://127.0.0.1:9/"
external_telegram_access = "FORBIDDEN"
real_user_lookup = "FORBIDDEN"
real_message_delivery = "FORBIDDEN"
"#
        .to_string()
    }

    fn write_environment_fixture(root: &Path, policy: &str) {
        fs::create_dir_all(root.join("proof")).unwrap();
        fs::create_dir_all(root.join(".github/workflows")).unwrap();
        fs::write(root.join("proof/policy.toml"), policy).unwrap();
        let workflow = r#"BOT_TOKEN: "1234567890:TEST_PLACEHOLDER_TOKEN_NOT_REAL"
ADMIN_USER_ID: "9000000000000000000"
ADMIN_CHAT_ID: "9000000000000000000"
TELEGRAM_API_URL: "http://127.0.0.1:9/"
"#;
        fs::write(root.join(".github/workflows/rust-ci.yml"), workflow).unwrap();
        fs::write(
            root.join(".github/workflows/auto-rusty-kaspa-update.yml"),
            workflow,
        )
        .unwrap();
    }

    #[test]
    fn environment_policy_is_fail_closed_against_weakened_or_unknown_state() {
        let baseline = canonical_environment_policy();
        let valid = tempfile::tempdir().unwrap();
        write_environment_fixture(valid.path(), &baseline);
        environment_boundary(valid.path()).expect("canonical policy must pass");

        for (from, to) in [
            (
                r#"production_source_mutation = "FORBIDDEN""#,
                r#"production_source_mutation = "ALLOWED""#,
            ),
            (
                r#"production_source_build = "FORBIDDEN""#,
                r#"production_source_build = "ALLOWED""#,
            ),
            (
                r#"production_as_ci_runner = "FORBIDDEN""#,
                r#"production_as_ci_runner = "ALLOWED""#,
            ),
            (
                r#"canonical_release_artifact = "PUBLISHED_ATTESTED_GITHUB_RELEASE""#,
                r#"canonical_release_artifact = "LOCAL_REBUILD""#,
            ),
            (
                r#"private_host_identity_in_public_policy = "FORBIDDEN""#,
                r#"private_host_identity_in_public_policy = "ALLOWED""#,
            ),
            (
                r#"external_telegram_access = "FORBIDDEN""#,
                r#"external_telegram_access = "ALLOWED""#,
            ),
            (
                r#"fixture_kind = "SYNTHETIC""#,
                r#"fixture_kind = "UNKNOWN""#,
            ),
            (r#"schema_version = "1.3.0""#, r#"schema_version = "9.9.9""#),
        ] {
            let dir = tempfile::tempdir().unwrap();
            write_environment_fixture(dir.path(), &baseline.replace(from, to));
            assert!(
                environment_boundary(dir.path()).is_err(),
                "weakened policy unexpectedly passed: {from} -> {to}"
            );
        }

        let missing = tempfile::tempdir().unwrap();
        write_environment_fixture(
            missing.path(),
            &baseline.replace(
                r#"production_source_build = "FORBIDDEN"
"#,
                "",
            ),
        );
        assert!(environment_boundary(missing.path()).is_err());
    }

    fn write_documentation_fixture(root: &Path) {
        write_environment_fixture(root, &canonical_environment_policy());
        fs::write(
            root.join(".github/workflows/security.yml"),
            "name: Security\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::write(root.join("docs/example.md"), "# Example\n").unwrap();
        fs::write(
            root.join("docs/OPERATIONAL_QUALIFICATION.md"),
            "# Operational qualification\nRun only in an isolated non-production environment.\n",
        )
        .unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nrust-version = \"1.97.1\"\n",
        )
        .unwrap();
        fs::write(
            root.join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.99.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("README.md"),
            "[![Rust](https://img.shields.io/badge/Rust-1.99.0-orange.svg)](https://www.rust-lang.org/)\n[Security](SECURITY.md)\n",
        )
        .unwrap();
        fs::write(root.join("SECURITY.md"), "# Security\n").unwrap();
        fs::write(
            root.join("SUPPLY_CHAIN.md"),
            "The proof schema is pinned to `1.3.0`.\n",
        )
        .unwrap();
        fs::write(root.join("CONTRIBUTING.md"), "# Contributing\n").unwrap();
        fs::write(root.join("AGENTS.md"), "# Agents\n").unwrap();
        fs::write(
            root.join("security-insights.yml"),
            "policy: \"https://github.com/KaspaPulse/kaspa-telegram-notify/blob/main/SECURITY.md\"\n",
        )
        .unwrap();
        fs::create_dir_all(root.join("opqual")).unwrap();
        fs::write(
            root.join("opqual/scenario-surfaces-v1.json"),
            r#"{
  "policies": {
    "fixture": {
      "critical_exact": ["README.md"],
      "critical_prefixes": ["docs/"],
      "proven_irrelevant_exact": ["SECURITY.md"],
      "proven_irrelevant_prefixes": [".github/workflows/"]
    }
  }
}"#,
        )
        .unwrap();
    }

    #[test]
    fn documentation_contract_fails_closed_on_dynamic_or_path_drift() {
        let valid = tempfile::tempdir().unwrap();
        write_documentation_fixture(valid.path());
        documentation(valid.path()).expect("canonical documentation fixture must pass");

        fs::write(
            valid.path().join("README.md"),
            "[![Rust](https://img.shields.io/badge/Rust-1.98.0-orange.svg)](https://www.rust-lang.org/)\n[Security](SECURITY.md)\n",
        )
        .unwrap();
        assert!(documentation(valid.path()).is_err());

        write_documentation_fixture(valid.path());
        fs::write(
            valid.path().join("README.md"),
            "[![Rust](https://img.shields.io/badge/Rust-1.99.0-orange.svg)](https://www.rust-lang.org/)\n[Missing](MISSING.md)\n",
        )
        .unwrap();
        assert!(documentation(valid.path()).is_err());

        write_documentation_fixture(valid.path());
        fs::write(
            valid.path().join("opqual/scenario-surfaces-v1.json"),
            r#"{
  "policies": {
    "fixture": {
      "critical_exact": ["MISSING.md"],
      "critical_prefixes": ["docs/"],
      "proven_irrelevant_exact": ["SECURITY.md"],
      "proven_irrelevant_prefixes": [".github/workflows/"]
    }
  }
}"#,
        )
        .unwrap();
        assert!(documentation(valid.path()).is_err());

        write_documentation_fixture(valid.path());
        fs::write(
            valid.path().join("AGENTS.md"),
            "# Agents\nProduction host: private-prod.internal\n",
        )
        .unwrap();
        assert!(documentation(valid.path()).is_err());
    }

    fn write_advisory_fixture(root: &Path) {
        let today = Utc::now().date_naive();
        let expiry = today + ChronoDuration::days(30);
        fs::create_dir_all(root.join("proof")).unwrap();
        fs::create_dir_all(root.join(".cargo")).unwrap();
        let reason = format!(
            "Reviewed {}: synthetic transitive fixture remains accepted only for fail-closed scanner consistency testing.",
            today.format("%Y-%m-%d")
        );
        fs::write(
            root.join("proof/policy.toml"),
            format!(
                r#"schema_version = "1.3.0"
policy_version = 4

[[advisory_exception]]
id = "RUSTSEC-2099-0001"
package = "fixture 1.0.0"
dependency_path = "fixture -> test"
direct_or_transitive = "TRANSITIVE"
runtime_or_build_time = "BUILD_TIME_TRANSITIVE"
target_reachability = "TEST_ONLY"
security_impact = "TEST_FIXTURE"
justification = "{reason}"
reviewed_at = "{reviewed}"
expires_at = "{expiry}"
remediation_trigger = "Delete the synthetic fixture after the test."
upstream_blocker_if_any = "Synthetic test fixture."
scanners = ["OSV", "CARGO_DENY"]
"#,
                reviewed = today.format("%Y-%m-%d"),
                expiry = expiry.format("%Y-%m-%d"),
            ),
        )
        .unwrap();
        fs::write(
            root.join(".cargo/audit.toml"),
            "[advisories]\nignore = []\n",
        )
        .unwrap();
        fs::write(
            root.join("deny.toml"),
            "[advisories]\nignore = [\"RUSTSEC-2099-0001\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("osv-scanner.toml"),
            format!(
                "[[IgnoredVulns]]\nid = \"RUSTSEC-2099-0001\"\nignoreUntil = {}\nreason = {:?}\n",
                expiry.format("%Y-%m-%d"),
                reason
            ),
        )
        .unwrap();
        fs::write(
            root.join("SECURITY_ADVISORIES.md"),
            "# Active\n\n### RUSTSEC-2099-0001 — fixture\n",
        )
        .unwrap();
    }

    #[test]
    fn advisory_policy_fails_closed_on_scanner_or_expiry_drift() {
        let dir = tempfile::tempdir().unwrap();
        write_advisory_fixture(dir.path());
        advisories(dir.path(), 45).expect("canonical advisory fixture must pass");

        fs::write(
            dir.path().join("deny.toml"),
            "[advisories]\nignore = [\"RUSTSEC-2099-0001\", \"RUSTSEC-2099-0002\"]\n",
        )
        .unwrap();
        assert!(advisories(dir.path(), 45).is_err());

        write_advisory_fixture(dir.path());
        let osv_path = dir.path().join("osv-scanner.toml");
        let osv = fs::read_to_string(&osv_path).unwrap();
        let tomorrow = (Utc::now().date_naive() + ChronoDuration::days(1))
            .format("%Y-%m-%d")
            .to_string();
        let expiry_pattern = Regex::new(r"ignoreUntil = \d{4}-\d{2}-\d{2}").unwrap();
        fs::write(
            &osv_path,
            expiry_pattern
                .replace(&osv, format!("ignoreUntil = {tomorrow}"))
                .as_ref(),
        )
        .unwrap();
        assert!(advisories(dir.path(), 45).is_err());
    }

    #[test]
    fn action_reference_policy_accepts_only_immutable_external_sha() {
        assert!(is_full_sha_action_reference(
            "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"
        ));
        assert!(is_full_sha_action_reference("./.github/actions/local"));
        assert!(!is_full_sha_action_reference("actions/checkout@v7"));
        assert!(!is_full_sha_action_reference(
            "google/osv-scanner-action/.github/workflows/osv-scanner-reusable.yml@v2.6.0"
        ));
    }
}
