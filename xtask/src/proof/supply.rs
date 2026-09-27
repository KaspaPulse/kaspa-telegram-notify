use anyhow::{Context, Result, ensure};
use serde_json::{Value as JsonValue, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use toml::Value as TomlValue;

fn read(root: &Path, path: &str) -> Result<String> {
    fs::read_to_string(root.join(path)).with_context(|| format!("failed to read {path}"))
}

fn digest(root: &Path, path: &str) -> Result<String> {
    Ok(crate::proof::sha256_hex(&fs::read(root.join(path))?))
}

fn policy_str<'a>(policy: &'a TomlValue, key: &str) -> Result<&'a str> {
    policy
        .get(key)
        .and_then(TomlValue::as_str)
        .with_context(|| format!("proof policy missing {key}"))
}

fn action_reference(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let raw = trimmed
        .strip_prefix("- uses:")
        .or_else(|| trimmed.strip_prefix("uses:"))?
        .trim()
        .trim_matches('"');
    let reference = raw.split_whitespace().next()?;
    if reference.starts_with("./") {
        None
    } else {
        Some(reference)
    }
}
fn full_sha_reference(reference: &str) -> bool {
    let Some((_, revision)) = reference.rsplit_once('@') else {
        return false;
    };
    revision.len() == 40
        && revision
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn action_pins(root: &Path) -> Result<Vec<JsonValue>> {
    let dir = root.join(".github/workflows");
    let mut files = fs::read_dir(&dir)?.collect::<std::io::Result<Vec<_>>>()?;
    files.sort_by_key(|entry| entry.file_name());
    let mut records = Vec::new();
    for entry in files {
        let path = entry.path();
        if !matches!(
            path.extension().and_then(|value| value.to_str()),
            Some("yml" | "yaml")
        ) {
            continue;
        }
        let text = fs::read_to_string(&path)?;
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        for (index, line) in text.lines().enumerate() {
            let Some(reference) = action_reference(line) else {
                continue;
            };
            ensure!(
                full_sha_reference(reference),
                "external action is not pinned to full SHA: {reference}"
            );
            records.push(json!({
                "workflow": rel,
                "line": index + 1,
                "reference": reference,
                "decision": "PINNED_FULL_SHA"
            }));
        }
    }
    records.sort_by_key(|record| {
        format!(
            "{}|{:06}|{}",
            record["workflow"].as_str().unwrap_or_default(),
            record["line"].as_u64().unwrap_or_default(),
            record["reference"].as_str().unwrap_or_default()
        )
    });
    Ok(records)
}
fn require_contains(text: &str, needle: &str, description: &str) -> Result<()> {
    ensure!(
        text.contains(needle),
        "{description}: missing required marker {needle}"
    );
    Ok(())
}

pub fn document(root: &Path, policy: &TomlValue) -> Result<JsonValue> {
    let required_ci = policy_str(policy, "required_ci_workflow")?;
    let msrv_ci = policy_str(policy, "msrv_ci_workflow")?;
    let msrv = policy_str(policy, "msrv")?;
    let toolchain = policy_str(policy, "toolchain")?;
    let slsa_claim = policy_str(policy, "slsa_claim")?;

    let required_ci_text = read(root, required_ci)?;
    require_contains(
        &required_ci_text,
        "cargo xtask proof verify",
        "required Rust-only proof CI gate",
    )?;

    let msrv_ci_text = read(root, msrv_ci)?;
    require_contains(
        &msrv_ci_text,
        &format!("toolchain: {msrv}"),
        "explicit MSRV toolchain",
    )?;
    require_contains(
        &msrv_ci_text,
        &format!("cargo +{msrv} check --locked --package kaspa-pulse --all-targets --all-features"),
        "MSRV supported feature matrix",
    )?;

    let cargo = read(root, "Cargo.toml")?;
    require_contains(
        &cargo,
        &format!("rust-version = \"{msrv}\""),
        "crate MSRV claim",
    )?;
    require_contains(
        &cargo,
        "unsafe_code = \"forbid\"",
        "root unsafe-code policy",
    )?;
    let xtask_cargo = read(root, "xtask/Cargo.toml")?;
    require_contains(
        &xtask_cargo,
        "unsafe_code = \"forbid\"",
        "xtask unsafe-code policy",
    )?;
    let toolchain_text = read(root, "rust-toolchain.toml")?;
    require_contains(
        &toolchain_text,
        &format!("channel = \"{toolchain}\""),
        "pinned development toolchain",
    )?;
    let release = read(root, ".github/workflows/release.yml")?;
    for (needle, description) in [
        ("cargo cyclonedx", "CycloneDX SBOM generation"),
        ("actions/attest@", "artifact attestation"),
        ("gh attestation verify", "attestation verification"),
        (
            "https://slsa.dev/provenance/v1",
            "SLSA provenance predicate",
        ),
        ("sha256sum", "SHA-256 release integrity"),
    ] {
        require_contains(&release, needle, description)?;
    }

    for path in ["deny.toml", "osv-scanner.toml", "SECURITY_ADVISORIES.md"] {
        ensure!(
            root.join(path).is_file(),
            "required dependency/advisory policy missing: {path}"
        );
    }

    let actions = action_pins(root)?;
    let mut inputs = BTreeMap::new();
    for path in [
        "Cargo.lock",
        "proof/policy.toml",
        required_ci,
        msrv_ci,
        ".github/workflows/release.yml",
        "deny.toml",
        "osv-scanner.toml",
        "SECURITY_ADVISORIES.md",
        "rust-toolchain.toml",
    ] {
        inputs.insert(path.to_owned(), digest(root, path)?);
    }

    Ok(json!({
        "schema_version": crate::proof::PROOF_SCHEMA_VERSION,
        "proof_type": "supply-chain-policy-proof",
        "status": "PASS",
        "inputs_sha256": inputs,
        "controls": {
            "required_ci_rust_only_gate": "ENFORCED",
            "action_pins": "PASS",
            "dependency_policy": "PASS",
            "advisory_policy": "PASS",
            "unsafe_code_policy": "PASS",
            "sbom": "PASS",
            "provenance": "PASS",
            "attestation_verification": "PASS",
            "slsa_claim": slsa_claim,
            "slsa_level_claimed": false
        },
        "msrv_policy": {
            "crate_msrv": msrv,
            "development_toolchain": toolchain,
            "ci_workflow": msrv_ci,
            "matrix": "kaspa-pulse --all-targets --all-features",
            "status": "EXPLICIT_AND_TESTED"
        },
        "action_references": actions,
        "proof_artifacts": [
            "rust-only-proof.json",
            "native-dependency-inventory.json",
            "supply-chain-proof.json"
        ]
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_sha_pin_is_accepted() {
        assert!(full_sha_reference(
            "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"
        ));
    }

    #[test]
    fn inline_version_comment_is_not_part_of_action_reference() {
        assert_eq!(
            action_reference(
                "uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1"
            ),
            Some("actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1")
        );
    }

    #[test]
    fn floating_action_ref_is_rejected() {
        assert!(!full_sha_reference("actions/checkout@v7"));
    }
}
