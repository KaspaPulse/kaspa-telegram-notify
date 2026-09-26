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

pub fn environment_boundary(root: impl AsRef<Path>) -> Result<()> {
    let root = root.as_ref();
    const MARKER: &str = "KAS_DNS_ENVIRONMENT_BOUNDARY_V1";
    let required: [(&str, &[&str]); 2] = [
        (
            "AGENTS.md",
            &[
                MARKER,
                "`kas` is the only development host.",
                "`dns` is the production host and is not a development environment.",
                "All source edits, dependency resolution, local Git operations, builds, tests, security scans, packaging, and release-artifact creation MUST run on `kas`.",
                "`dns` MUST NOT be configured or used as a CI runner.",
                "No source build is permitted on `dns` as part of deployment.",
                "Do not repair source code in place on production.",
            ],
        ),
        (
            "CONTRIBUTING.md",
            &[
                MARKER,
                "Development host: `kas` only.",
                "Production host: `dns` is deployment/verification only.",
            ],
        ),
    ];

    for (file, needles) in required {
        let text = read_text(root, file)?;
        let missing: Vec<_> = needles
            .iter()
            .copied()
            .filter(|needle| !text.contains(needle))
            .collect();
        ensure!(
            missing.is_empty(),
            "environment-boundary-check: {file} missing required policy: {missing:?}"
        );
    }

    println!("environment-boundary-check: PASS");
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
    let audit = read_text(root, ".cargo/audit.toml")?;
    let osv_text = read_text(root, "osv-scanner.toml")?;
    let report = read_text(root, "SECURITY_ADVISORIES.md")?;

    let rustsec = Regex::new(r"RUSTSEC-\d{4}-\d{4}")?;
    let ignored_ids: BTreeSet<_> = rustsec
        .find_iter(&audit)
        .map(|m| m.as_str().to_owned())
        .collect();
    let undocumented: Vec<_> = ignored_ids
        .iter()
        .filter(|id| !report.contains(id.as_str()))
        .cloned()
        .collect();
    ensure!(
        undocumented.is_empty(),
        "undocumented cargo-audit ignored RustSec IDs: {}",
        undocumented.join(", ")
    );

    let review_pattern = Regex::new(r"Last automated review:\s*\*\*(\d{4}-\d{2}-\d{2})\*\*")?;
    let reviewed = review_pattern
        .captures(&report)
        .and_then(|captures| captures.get(1))
        .context("SECURITY_ADVISORIES.md is missing 'Last automated review: **YYYY-MM-DD**'")?;
    let reviewed = NaiveDate::parse_from_str(reviewed.as_str(), "%Y-%m-%d")
        .context("invalid security advisory review date")?;
    let today = Utc::now().date_naive();
    let age_days = (today - reviewed).num_days();
    ensure!(
        age_days >= 0,
        "security advisory review date is in the future"
    );
    ensure!(
        age_days <= max_age_days,
        "security advisory review is {age_days} days old; maximum allowed age is {max_age_days} days"
    );

    let osv: TomlValue = osv_text.parse().context("cannot parse osv-scanner.toml")?;
    let entries = osv
        .get("IgnoredVulns")
        .and_then(TomlValue::as_array)
        .context("osv-scanner.toml IgnoredVulns must be an array of tables")?;

    let latest_allowed = today + ChronoDuration::days(max_age_days);
    let mut osv_ids = BTreeSet::new();
    for (index, entry) in entries.iter().enumerate() {
        let table = entry
            .as_table()
            .with_context(|| format!("IgnoredVulns entry #{} must be a table", index + 1))?;
        let advisory = table
            .get("id")
            .and_then(TomlValue::as_str)
            .with_context(|| format!("IgnoredVulns entry #{} must define id", index + 1))?;
        ensure!(
            rustsec
                .find(advisory)
                .is_some_and(|found| found.as_str() == advisory),
            "IgnoredVulns entry #{} must use a concrete RUSTSEC-YYYY-NNNN id",
            index + 1
        );
        ensure!(
            osv_ids.insert(advisory.to_owned()),
            "duplicate OSV ignored advisory: {advisory}"
        );
        ensure!(
            report.contains(advisory),
            "OSV ignored advisory is not documented in SECURITY_ADVISORIES.md: {advisory}"
        );
        let reason = table
            .get("reason")
            .and_then(TomlValue::as_str)
            .unwrap_or_default();
        ensure!(
            reason.trim().chars().count() >= 40,
            "{advisory} reason must contain at least 40 non-whitespace characters"
        );
        let expiry = date_from_toml(
            table
                .get("ignoreUntil")
                .with_context(|| format!("{advisory} must define ignoreUntil"))?,
            advisory,
        )?;
        ensure!(
            expiry >= today,
            "OSV exception expired for {advisory} on {expiry}"
        );
        ensure!(
            expiry <= latest_allowed,
            "OSV exception for {advisory} expires {expiry}, more than {max_age_days} days from today"
        );
    }

    println!(
        "security-advisory-check: PASS ({} cargo-audit ignores documented; {} time-bounded OSV exceptions validated; review age {} days)",
        ignored_ids.len(),
        osv_ids.len(),
        age_days
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
    fn scorecard_filter_fails_closed_on_malformed_input() {
        let mut document = serde_json::json!({});
        assert!(filter_scorecard_sarif(&mut document).is_err());
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
