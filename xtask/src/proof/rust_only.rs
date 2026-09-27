use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value as JsonValue, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Clone, Copy, Debug)]
struct Class {
    ownership: &'static str,
    role: &'static str,
    file_class: &'static str,
    origin: &'static str,
    target_relevance: &'static str,
    decision: &'static str,
    implementation: bool,
}

fn known(
    ownership: &'static str,
    role: &'static str,
    file_class: &'static str,
    origin: &'static str,
    target_relevance: &'static str,
    decision: &'static str,
    implementation: bool,
) -> Class {
    Class {
        ownership,
        role,
        file_class,
        origin,
        target_relevance,
        decision,
        implementation,
    }
}

fn classify_known(path: &str) -> Option<Class> {
    let ext = Path::new(path).extension().and_then(|value| value.to_str());
    if path.starts_with("src/") && ext == Some("rs") {
        return Some(known(
            "OWNED",
            "RUNTIME_IMPLEMENTATION",
            "RUST_SOURCE",
            "FIRST_PARTY",
            "PRODUCTION",
            "KNOWN_AND_ALLOWED",
            true,
        ));
    }
    if path.starts_with("tests/") && ext == Some("rs") {
        return Some(known(
            "OWNED",
            "TEST_QUALIFICATION",
            "RUST_SOURCE",
            "FIRST_PARTY",
            "TEST",
            "KNOWN_AND_ALLOWED",
            true,
        ));
    }
    if path.starts_with("xtask/src/") && ext == Some("rs") {
        return Some(known(
            "OWNED",
            "OPERATIONAL_TOOLING",
            "RUST_SOURCE",
            "FIRST_PARTY",
            "BUILD_CI_OPERATIONS",
            "KNOWN_AND_ALLOWED",
            true,
        ));
    }
    if path.starts_with("opqual-fixture/src/") && ext == Some("rs") {
        return Some(known(
            "OWNED",
            "TEST_FIXTURE",
            "RUST_SOURCE",
            "FIRST_PARTY",
            "TEST",
            "KNOWN_AND_ALLOWED",
            true,
        ));
    }
    if path.starts_with("fuzz/fuzz_targets/") && ext == Some("rs") {
        return Some(known(
            "OWNED",
            "SECURITY_TESTING",
            "RUST_SOURCE",
            "FIRST_PARTY",
            "SECURITY_TEST",
            "KNOWN_AND_ALLOWED",
            true,
        ));
    }
    if path.ends_with("Cargo.toml") {
        return Some(known(
            "OWNED",
            "BUILD_CONFIGURATION",
            "CARGO_MANIFEST",
            "FIRST_PARTY",
            "BUILD",
            "KNOWN_AND_ALLOWED",
            false,
        ));
    }
    if path == "Cargo.lock" {
        return Some(known(
            "OWNED",
            "DEPENDENCY_LOCK",
            "CARGO_LOCK",
            "FIRST_PARTY",
            "BUILD_SECURITY",
            "KNOWN_AND_ALLOWED",
            false,
        ));
    }
    if path == "proof/policy.toml" {
        return Some(known(
            "OWNED",
            "PROOF_POLICY",
            "CONFIGURATION",
            "FIRST_PARTY",
            "PROOF",
            "KNOWN_AND_POLICY_APPROVED",
            false,
        ));
    }
    if path.starts_with(".sqlx/") && ext == Some("json") {
        return Some(known(
            "GENERATED",
            "BUILD_METADATA",
            "JSON_DATA",
            "TOOL_OUTPUT",
            "BUILD",
            "KNOWN_AND_POLICY_APPROVED",
            false,
        ));
    }
    if path.starts_with("migrations/") && ext == Some("sql") {
        return Some(known(
            "OWNED",
            "DATABASE_MIGRATION",
            "SQL_MIGRATION",
            "FIRST_PARTY",
            "PRODUCTION",
            "KNOWN_AND_ALLOWED",
            false,
        ));
    }
    if path.starts_with(".github/workflows/") && matches!(ext, Some("yml" | "yaml")) {
        return Some(known(
            "OWNED",
            "CI_WORKFLOW",
            "WORKFLOW_YAML",
            "FIRST_PARTY",
            "CI",
            "KNOWN_AND_ALLOWED",
            false,
        ));
    }
    if path.starts_with(".github/") {
        return Some(known(
            "OWNED",
            "REPOSITORY_GOVERNANCE",
            "REPOSITORY_METADATA",
            "FIRST_PARTY",
            "GOVERNANCE",
            "KNOWN_AND_ALLOWED",
            false,
        ));
    }
    if path.starts_with("ops/nginx/") && ext == Some("conf") {
        return Some(known(
            "OWNED",
            "OPERATIONAL_CONFIGURATION",
            "CONFIGURATION",
            "FIRST_PARTY",
            "OPERATIONS",
            "KNOWN_AND_ALLOWED",
            false,
        ));
    }
    if path == "opqual/scenario-map.csv" {
        return Some(known(
            "OWNED",
            "TEST_DATA",
            "DATA",
            "FIRST_PARTY",
            "TEST",
            "KNOWN_AND_ALLOWED",
            false,
        ));
    }
    if path == "opqual/scenario-surfaces-v1.json" {
        return Some(known(
            "OWNED",
            "E2E_IMPACT_POLICY",
            "CONFIGURATION",
            "FIRST_PARTY",
            "TEST_CI",
            "KNOWN_AND_POLICY_APPROVED",
            false,
        ));
    }
    if ext == Some("md") {
        return Some(known(
            "OWNED",
            "DOCUMENTATION",
            "DOCUMENTATION",
            "FIRST_PARTY",
            "DOCUMENTATION",
            "KNOWN_AND_ALLOWED",
            false,
        ));
    }
    if ext == Some("toml") {
        return Some(known(
            "OWNED",
            "POLICY_CONFIGURATION",
            "CONFIGURATION",
            "FIRST_PARTY",
            "BUILD_SECURITY",
            "KNOWN_AND_ALLOWED",
            false,
        ));
    }
    if matches!(ext, Some("yml" | "yaml")) {
        return Some(known(
            "OWNED",
            "REPOSITORY_CONFIGURATION",
            "CONFIGURATION",
            "FIRST_PARTY",
            "GOVERNANCE",
            "KNOWN_AND_ALLOWED",
            false,
        ));
    }
    if [
        ".gitignore",
        ".dockerignore",
        ".env.example",
        "fuzz/.gitignore",
        "Dockerfile",
        "LICENSE",
    ]
    .contains(&path)
    {
        return Some(known(
            "OWNED",
            "REPOSITORY_CONFIGURATION",
            "REPOSITORY_METADATA",
            "FIRST_PARTY",
            "BUILD_OPERATIONS",
            "KNOWN_AND_ALLOWED",
            false,
        ));
    }
    None
}

fn classify_artifact(path: &str, mode: &str, bytes: &[u8]) -> Result<Class> {
    if path.starts_with("vendor/") {
        bail!("vendored or external artifact requires explicit policy approval: {path}");
    }
    if path.starts_with("generated/") {
        bail!("generated artifact requires explicit policy approval: {path}");
    }
    if mode == "100755" {
        bail!("unclassified tracked executable artifact: {path}");
    }
    if bytes.starts_with(b"#!")
        && Path::new(path).extension().and_then(|value| value.to_str()) != Some("rs")
    {
        bail!("non-Rust shebang is not allowed in owned tracked artifacts: {path}");
    }
    let class = classify_known(path)
        .with_context(|| format!("unknown tracked artifact classification: {path}"))?;
    ensure!(
        !class.implementation || class.file_class == "RUST_SOURCE",
        "owned implementation must be Rust: {path}"
    );
    Ok(class)
}

fn git_output(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .context("failed to execute git")?;
    ensure!(output.status.success(), "git command failed");
    Ok(output.stdout)
}

fn tracked_modes(root: &Path) -> Result<std::collections::BTreeMap<String, String>> {
    let output = git_output(root, &["ls-files", "-s", "-z"])?;
    let text = String::from_utf8(output).context("git index paths are not UTF-8")?;
    let mut modes = std::collections::BTreeMap::new();
    for entry in text.split('\0').filter(|value| !value.is_empty()) {
        let (meta, path) = entry
            .split_once('\t')
            .context("invalid git ls-files -s entry")?;
        let mode = meta
            .split_whitespace()
            .next()
            .context("missing git file mode")?;
        modes.insert(path.to_owned(), mode.to_owned());
    }
    Ok(modes)
}

#[cfg(unix)]
fn untracked_mode(path: &Path) -> Result<String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = fs::metadata(path)?.permissions().mode();
    Ok(if mode & 0o111 == 0 {
        "100644"
    } else {
        "100755"
    }
    .to_owned())
}
#[cfg(not(unix))]
fn untracked_mode(_path: &Path) -> Result<String> {
    bail!("untracked artifact executability cannot be proven on this platform")
}

pub fn document(root: &Path) -> Result<JsonValue> {
    let modes = tracked_modes(root)?;
    let output = git_output(
        root,
        &[
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ],
    )?;
    let text = String::from_utf8(output).context("git paths are not UTF-8")?;
    let mut paths = text
        .split('\0')
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    let mut input = Sha256::new();
    let mut records = Vec::with_capacity(paths.len());
    for path in paths {
        let full = root.join(&path);
        let bytes = fs::read(&full)
            .with_context(|| format!("failed to read tracked/relevant artifact: {path}"))?;
        let mode = modes
            .get(&path)
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| untracked_mode(&full))?;
        let class = classify_artifact(&path, &mode, &bytes)?;
        let content_sha = crate::proof::sha256_hex(&bytes);
        input.update(mode.as_bytes());
        input.update([0]);
        input.update(path.as_bytes());
        input.update([0]);
        input.update(content_sha.as_bytes());
        input.update([0]);
        records.push(json!({
            "path": path,
            "mode": mode,
            "content_sha256": content_sha,
            "ownership": class.ownership,
            "role": class.role,
            "file_class": class.file_class,
            "executable": mode == "100755",
            "origin": class.origin,
            "target_relevance": class.target_relevance,
            "decision": class.decision,
            "implementation": class.implementation
        }));
    }
    let rust_source_files = records
        .iter()
        .filter(|item| item["file_class"] == "RUST_SOURCE")
        .count();
    let executable_artifacts = records
        .iter()
        .filter(|item| item["executable"] == true)
        .count();
    Ok(json!({
        "schema_version": crate::proof::PROOF_SCHEMA_VERSION,
        "proof_type": "owned-source-rust-only",
        "status": "PASS",
        "input_digest_sha256": crate::proof::hex_bytes(input.finalize()),
        "classification_model": ["OWNERSHIP", "ROLE", "PATH", "FILE_CLASS", "EXECUTABILITY", "ORIGIN", "TARGET_RELEVANCE"],
        "decision_policy": {"KNOWN_AND_ALLOWED": "PASS", "KNOWN_AND_POLICY_APPROVED": "PASS", "UNKNOWN": "FAIL"},
        "summary": {
            "relevant_artifacts": records.len(),
            "rust_source_files": rust_source_files,
            "executable_artifacts": executable_artifacts,
            "unknown_classifications": 0,
            "owned_non_rust_implementation": 0
        },
        "artifacts": records
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_rust_source_is_allowed() {
        let class = classify_artifact("src/main.rs", "100644", b"fn main() {}").unwrap();
        assert!(class.implementation);
        assert_eq!(class.file_class, "RUST_SOURCE");
    }

    #[test]
    fn e2e_surface_manifest_is_explicitly_policy_approved() {
        let class = classify_artifact(
            "opqual/scenario-surfaces-v1.json",
            "100644",
            br#"{"schema_version":1}"#,
        )
        .unwrap();
        assert_eq!(class.role, "E2E_IMPACT_POLICY");
        assert_eq!(class.file_class, "CONFIGURATION");
        assert_eq!(class.target_relevance, "TEST_CI");
        assert_eq!(class.decision, "KNOWN_AND_POLICY_APPROVED");
        assert!(!class.implementation);
    }

    #[test]
    fn adversarial_artifacts_fail_closed() {
        let cases: [(&str, &str, &[u8]); 9] = [
            ("owned/foo.py", "100644", b"print('x')"),
            ("owned/foo.sh", "100644", b"echo x"),
            ("owned/tool", "100755", b"binary"),
            ("owned/tool.xyz", "100644", b"#!/usr/bin/python\n"),
            ("ops/helper", "100755", b"tool"),
            ("build/helper", "100755", b"tool"),
            ("generated/binary", "100644", b"artifact"),
            ("vendor/native.c", "100644", b"int x;"),
            ("owned/future.unknown", "100644", b"data"),
        ];
        for (path, mode, bytes) in cases {
            assert!(
                classify_artifact(path, mode, bytes).is_err(),
                "must fail closed: {path}"
            );
        }
    }
}
