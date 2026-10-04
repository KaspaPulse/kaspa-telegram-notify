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

fn validate_toolchain_contract(root: &Path, policy: &TomlValue) -> Result<()> {
    let product_msrv = policy_str(policy, "msrv")?;
    let xtask_msrv = policy_str(policy, "xtask_msrv")?;
    let opqual_msrv = policy_str(policy, "opqual_fixture_msrv")?;
    let toolchain = policy_str(policy, "toolchain")?;
    let fuzz_nightly = policy_str(policy, "fuzz_nightly")?;
    let docker_builder = policy_str(policy, "docker_builder_image")?;

    for (path, needle, description) in [
        (
            "Cargo.toml".to_string(),
            format!("rust-version = \"{product_msrv}\""),
            "product package MSRV".to_string(),
        ),
        (
            "xtask/Cargo.toml".to_string(),
            format!("rust-version = \"{xtask_msrv}\""),
            "xtask package MSRV".to_string(),
        ),
        (
            "opqual-fixture/Cargo.toml".to_string(),
            format!("rust-version = \"{opqual_msrv}\""),
            "opqual-fixture package MSRV".to_string(),
        ),
        (
            "rust-toolchain.toml".to_string(),
            format!("channel = \"{toolchain}\""),
            "primary development toolchain".to_string(),
        ),
        (
            "Dockerfile".to_string(),
            format!("FROM {docker_builder} AS builder"),
            "verified Docker builder image".to_string(),
        ),
        (
            "README.md".to_string(),
            format!("Rust-{toolchain}-orange"),
            "README primary toolchain badge".to_string(),
        ),
    ] {
        require_contains(&read(root, &path)?, &needle, &description)?;
    }

    let fuzz = read(root, ".github/workflows/fuzz.yml")?;
    require_contains(
        &fuzz,
        &format!("toolchain: {fuzz_nightly}"),
        "pinned fuzz nightly",
    )?;
    require_contains(
        &fuzz,
        &format!("cargo +{fuzz_nightly} fuzz run"),
        "fuzz nightly execution",
    )?;

    for workflow in [
        ".github/workflows/rust-ci.yml",
        ".github/workflows/security.yml",
        ".github/workflows/hermetic-e2e.yml",
        ".github/workflows/release.yml",
        ".github/workflows/scorecard.yml",
        ".github/workflows/workflow-lint.yml",
        ".github/workflows/auto-rusty-kaspa-update.yml",
        ".github/workflows/external-contract-e2e.yml",
    ] {
        require_contains(
            &read(root, workflow)?,
            &format!("toolchain: {toolchain}"),
            &format!("primary toolchain in {workflow}"),
        )?;
    }

    let ci = read(root, ".github/workflows/rust-ci.yml")?;
    for (needle, description) in [
        (
            format!(
                "cargo +{product_msrv} check --locked --package kaspa-pulse --all-targets --all-features"
            ),
            "product MSRV compile verification",
        ),
        (
            format!(
                "cargo +{product_msrv} test --locked --package kaspa-pulse --all-targets --all-features"
            ),
            "product MSRV functional verification",
        ),
        (
            format!(
                "cargo +{xtask_msrv} check --locked --package xtask --all-targets --all-features"
            ),
            "xtask MSRV compile verification",
        ),
        (
            format!(
                "cargo +{xtask_msrv} test --locked --package xtask --all-targets --all-features"
            ),
            "xtask MSRV functional verification",
        ),
        (
            format!(
                "cargo +{opqual_msrv} check --locked --package opqual-fixture --all-targets --all-features"
            ),
            "opqual-fixture MSRV compile verification",
        ),
        (
            format!(
                "cargo +{opqual_msrv} test --locked --package opqual-fixture --all-targets --all-features"
            ),
            "opqual-fixture MSRV functional verification",
        ),
        (
            "cargo hack check --rust-version --workspace --all-targets --all-features --locked"
                .to_string(),
            "cargo-hack rust-version contract",
        ),
    ] {
        require_contains(&ci, &needle, description)?;
    }

    let release = read(root, ".github/workflows/release.yml")?;
    for (needle, description) in [
        ("queue: max", "release concurrency queue"),
        (
            "Classify release state before artifact generation",
            "early release-state gate",
        ),
        (
            "RELEASE_LOOKUP_CLASSIFICATION_SELF_TEST=PASS",
            "release-state negative self-test",
        ),
        (
            "RELEASE_BARRIER_NEGATIVE_TEST=PASS",
            "release-barrier negative self-test",
        ),
        (
            "gh release create \"$TAG\" --draft",
            "draft release creation",
        ),
        (
            "gh release edit \"$TAG\" --draft=false",
            "draft publication boundary",
        ),
        (
            "gh release verify \"$TAG\"",
            "GitHub-native release attestation verification",
        ),
        (
            "gh release verify-asset \"$TAG\"",
            "GitHub-native asset verification",
        ),
    ] {
        require_contains(&release, needle, description)?;
    }

    Ok(())
}

pub fn document(root: &Path, policy: &TomlValue) -> Result<JsonValue> {
    let required_ci = policy_str(policy, "required_ci_workflow")?;
    let msrv_ci = policy_str(policy, "msrv_ci_workflow")?;
    let msrv = policy_str(policy, "msrv")?;
    let xtask_msrv = policy_str(policy, "xtask_msrv")?;
    let opqual_msrv = policy_str(policy, "opqual_fixture_msrv")?;
    let toolchain = policy_str(policy, "toolchain")?;
    let fuzz_nightly = policy_str(policy, "fuzz_nightly")?;
    let docker_builder = policy_str(policy, "docker_builder_image")?;
    let slsa_claim = policy_str(policy, "slsa_claim")?;
    validate_toolchain_contract(root, policy)?;
    let environment = policy
        .get("environment_boundary")
        .and_then(TomlValue::as_table)
        .context("proof policy missing [environment_boundary]")?;
    let ci_identity = policy
        .get("ci_identity")
        .and_then(TomlValue::as_table)
        .context("proof policy missing [ci_identity]")?;
    let advisory_count = policy
        .get("advisory_exception")
        .and_then(TomlValue::as_array)
        .context("proof policy missing advisory_exception records")?
        .len();

    let env_str = |key: &str| -> Result<&str> {
        environment
            .get(key)
            .and_then(TomlValue::as_str)
            .with_context(|| format!("environment_boundary missing {key}"))
    };
    ensure!(env_str("production_source_mutation")? == "FORBIDDEN");
    ensure!(env_str("production_source_build")? == "FORBIDDEN");
    ensure!(env_str("production_as_ci_runner")? == "FORBIDDEN");
    ensure!(env_str("local_build_role")? == "QUALIFICATION_ONLY");
    ensure!(env_str("canonical_release_artifact")? == "PUBLISHED_ATTESTED_GITHUB_RELEASE");
    ensure!(env_str("private_host_identity_in_public_policy")? == "FORBIDDEN");

    let ci_str = |key: &str| -> Result<&str> {
        ci_identity
            .get(key)
            .and_then(TomlValue::as_str)
            .with_context(|| format!("ci_identity missing {key}"))
    };
    ensure!(ci_str("fixture_kind")? == "SYNTHETIC");
    ensure!(ci_str("external_telegram_access")? == "FORBIDDEN");
    ensure!(ci_str("real_user_lookup")? == "FORBIDDEN");
    ensure!(ci_str("real_message_delivery")? == "FORBIDDEN");

    let required_ci_text = read(root, required_ci)?;
    require_contains(
        &required_ci_text,
        "cargo xtask proof verify",
        "required Rust-only proof CI gate",
    )?;
    require_contains(
        &required_ci_text,
        "cargo xtask security environment-boundary",
        "structured environment-boundary CI gate",
    )?;
    require_contains(
        &required_ci_text,
        "cargo xtask security documentation",
        "documentation-contract CI gate",
    )?;
    require_contains(
        &required_ci_text,
        "cargo xtask security advisories --max-age-days 45",
        "canonical advisory CI gate",
    )?;

    let msrv_ci_text = read(root, msrv_ci)?;
    require_contains(
        &msrv_ci_text,
        &format!("rustup toolchain install {msrv} --profile minimal"),
        "explicit product MSRV toolchain installation",
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
        "Dockerfile",
        "README.md",
        "xtask/Cargo.toml",
        "opqual-fixture/Cargo.toml",
        ".github/workflows/fuzz.yml",
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
            "slsa_level_claimed": false,
            "toolchain_contract": "PASS",
            "release_workflow_contract": "PASS"
        },
        "msrv_policy": {
            "crate_msrv": msrv,
            "xtask_msrv": xtask_msrv,
            "opqual_fixture_msrv": opqual_msrv,
            "development_toolchain": toolchain,
            "fuzz_nightly": fuzz_nightly,
            "docker_builder_image": docker_builder,
            "ci_workflow": msrv_ci,
            "matrix": "workspace package-scoped --all-targets --all-features",
            "status": "EXPLICIT_AND_FUNCTIONALLY_TESTED"
        },
        "environment_boundary": {
            "production_source_mutation": env_str("production_source_mutation")?,
            "production_source_build": env_str("production_source_build")?,
            "production_as_ci_runner": env_str("production_as_ci_runner")?,
            "local_build_role": env_str("local_build_role")?,
            "canonical_release_artifact": env_str("canonical_release_artifact")?,
            "private_host_identity_in_public_policy": env_str("private_host_identity_in_public_policy")?
        },
        "ci_identity": {
            "fixture_kind": ci_str("fixture_kind")?,
            "telegram_api_url": ci_str("telegram_api_url")?,
            "external_telegram_access": ci_str("external_telegram_access")?,
            "real_user_lookup": ci_str("real_user_lookup")?,
            "real_message_delivery": ci_str("real_message_delivery")?
        },
        "advisory_policy": {
            "canonical_record_count": advisory_count,
            "authority": "proof/policy.toml",
            "scanner_projection": "FAIL_CLOSED"
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
    fn write_toolchain_fixture(root: &Path) {
        fs::create_dir_all(root.join("proof")).unwrap();
        fs::create_dir_all(root.join(".github/workflows")).unwrap();
        fs::create_dir_all(root.join("xtask")).unwrap();
        fs::create_dir_all(root.join("opqual-fixture")).unwrap();
        fs::write(
            root.join("proof/policy.toml"),
            r#"schema_version = "1.3.0"
policy_version = 4
msrv = "1.97.1"
xtask_msrv = "1.98.1"
opqual_fixture_msrv = "1.98.1"
toolchain = "1.99.0"
fuzz_nightly = "nightly-2026-08-01"
docker_builder_image = "rust:1.99.0-slim-trixie@sha256:test"
"#,
        )
        .unwrap();
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nname=\"x\"\nversion=\"0.1.0\"\nrust-version = \"1.97.1\"\n",
        )
        .unwrap();
        fs::write(
            root.join("xtask/Cargo.toml"),
            "[package]\nname=\"xtask\"\nversion=\"0.1.0\"\nrust-version = \"1.98.1\"\n",
        )
        .unwrap();
        fs::write(
            root.join("opqual-fixture/Cargo.toml"),
            "[package]\nname=\"opqual-fixture\"\nversion=\"0.1.0\"\nrust-version = \"1.98.1\"\n",
        )
        .unwrap();
        fs::write(
            root.join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.99.0\"\n",
        )
        .unwrap();
        fs::write(
            root.join("Dockerfile"),
            "FROM rust:1.99.0-slim-trixie@sha256:test AS builder\n",
        )
        .unwrap();
        fs::write(root.join("README.md"), "Rust-1.99.0-orange\n").unwrap();
        fs::write(
            root.join(".github/workflows/fuzz.yml"),
            "toolchain: nightly-2026-08-01\nrun: cargo +nightly-2026-08-01 fuzz run x\n",
        )
        .unwrap();
        for name in [
            "security.yml",
            "hermetic-e2e.yml",
            "scorecard.yml",
            "workflow-lint.yml",
            "auto-rusty-kaspa-update.yml",
            "external-contract-e2e.yml",
        ] {
            fs::write(
                root.join(".github/workflows").join(name),
                "toolchain: 1.99.0\n",
            )
            .unwrap();
        }
        fs::write(
            root.join(".github/workflows/rust-ci.yml"),
            concat!(
                "toolchain: 1.99.0\n",
                "cargo +1.97.1 check --locked --package kaspa-pulse --all-targets --all-features\n",
                "cargo +1.97.1 test --locked --package kaspa-pulse --all-targets --all-features\n",
                "cargo +1.98.1 check --locked --package xtask --all-targets --all-features\n",
                "cargo +1.98.1 test --locked --package xtask --all-targets --all-features\n",
                "cargo +1.98.1 check --locked --package opqual-fixture --all-targets --all-features\n",
                "cargo +1.98.1 test --locked --package opqual-fixture --all-targets --all-features\n",
                "cargo hack check --rust-version --workspace --all-targets --all-features --locked\n"
            ),
        ).unwrap();
        fs::write(
            root.join(".github/workflows/release.yml"),
            concat!(
                "toolchain: 1.99.0\nqueue: max\n",
                "Classify release state before artifact generation\n",
                "RELEASE_LOOKUP_CLASSIFICATION_SELF_TEST=PASS\n",
                "RELEASE_BARRIER_NEGATIVE_TEST=PASS\n",
                "gh release create \"$TAG\" --draft\n",
                "gh release edit \"$TAG\" --draft=false\n",
                "gh release verify \"$TAG\"\n",
                "gh release verify-asset \"$TAG\"\n"
            ),
        )
        .unwrap();
    }

    #[test]
    fn toolchain_contract_fails_closed_on_primary_or_msrv_drift() {
        let dir = tempfile::tempdir().unwrap();
        write_toolchain_fixture(dir.path());
        let policy: TomlValue = fs::read_to_string(dir.path().join("proof/policy.toml"))
            .unwrap()
            .parse()
            .unwrap();
        validate_toolchain_contract(dir.path(), &policy)
            .expect("canonical toolchain fixture must pass");

        fs::write(
            dir.path().join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.98.1\"\n",
        )
        .unwrap();
        assert!(validate_toolchain_contract(dir.path(), &policy).is_err());

        write_toolchain_fixture(dir.path());
        fs::write(
            dir.path().join("xtask/Cargo.toml"),
            "[package]\nname=\"xtask\"\nversion=\"0.1.0\"\nrust-version = \"1.97.1\"\n",
        )
        .unwrap();
        assert!(validate_toolchain_contract(dir.path(), &policy).is_err());

        write_toolchain_fixture(dir.path());
        fs::write(
            dir.path().join("Dockerfile"),
            "FROM rust:1.99.0-slim-trixie@sha256:wrong AS builder\n",
        )
        .unwrap();
        assert!(validate_toolchain_contract(dir.path(), &policy).is_err());
    }
}
