mod native;
mod rust_only;
mod supply;

use anyhow::{Context, Result, ensure};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use toml::Value as TomlValue;

pub const PROOF_SCHEMA_VERSION: &str = "1.3.0";
const RUST_ONLY_FILE: &str = "rust-only-proof.json";
const NATIVE_FILE: &str = "native-dependency-inventory.json";
const SUPPLY_FILE: &str = "supply-chain-proof.json";

fn policy(root: &Path) -> Result<TomlValue> {
    let text = fs::read_to_string(root.join("proof/policy.toml"))
        .context("failed to read proof/policy.toml")?;
    let value: TomlValue = text.parse().context("invalid proof/policy.toml")?;
    ensure!(
        value.get("schema_version").and_then(TomlValue::as_str) == Some(PROOF_SCHEMA_VERSION),
        "proof schema version must be pinned to {PROOF_SCHEMA_VERSION}"
    );
    ensure!(
        value.get("policy_version").and_then(TomlValue::as_integer) == Some(4),
        "proof policy version must be pinned to 4"
    );
    Ok(value)
}

fn artifact_dir(root: &Path, policy: &TomlValue) -> Result<PathBuf> {
    let rel = policy
        .get("proof_artifact_dir")
        .and_then(TomlValue::as_str)
        .context("proof_artifact_dir missing")?;
    Ok(root.join(rel))
}
pub(super) fn hex_bytes(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    hex_bytes(Sha256::digest(bytes))
}

fn json_bytes(value: &JsonValue) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn documents(root: &Path) -> Result<Vec<(&'static str, JsonValue)>> {
    let policy = policy(root)?;
    let rust = rust_only::document(root)?;
    let native_doc = native::document(root, &policy)?;
    let supply_doc = supply::document(root, &policy)?;
    Ok(vec![
        (RUST_ONLY_FILE, rust),
        (NATIVE_FILE, native_doc),
        (SUPPLY_FILE, supply_doc),
    ])
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("proof artifact parent missing")?;
    fs::create_dir_all(parent)?;
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .context("proof artifact filename is not UTF-8")?;
    let tmp = parent.join(format!(".{name}.tmp-{}", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .with_context(|| format!("failed to create {}", tmp.display()))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&tmp, path)?;
    let readback = fs::read(path)?;
    ensure!(
        readback == bytes,
        "proof artifact read-back mismatch: {}",
        path.display()
    );
    Ok(())
}
fn write_documents(
    root: &Path,
    docs: &[(&'static str, JsonValue)],
) -> Result<Vec<(String, String)>> {
    let policy = policy(root)?;
    let dir = artifact_dir(root, &policy)?;
    let mut written = Vec::new();
    for (name, document) in docs {
        let bytes = json_bytes(document)?;
        let path = dir.join(name);
        write_atomic(&path, &bytes)?;
        let digest = sha256_hex(&bytes);
        written.push((name.to_string(), digest));
    }
    Ok(written)
}

fn emit(root: &Path, filename: &str) -> Result<()> {
    let (_, document) = documents(root)?
        .into_iter()
        .find(|(name, _)| *name == filename)
        .context("unknown proof artifact")?;
    print!("{}", String::from_utf8(json_bytes(&document)?)?);
    Ok(())
}

pub fn emit_rust_only(root: impl AsRef<Path>) -> Result<()> {
    emit(root.as_ref(), RUST_ONLY_FILE)
}

pub fn emit_native(root: impl AsRef<Path>) -> Result<()> {
    emit(root.as_ref(), NATIVE_FILE)
}

pub fn emit_supply(root: impl AsRef<Path>) -> Result<()> {
    emit(root.as_ref(), SUPPLY_FILE)
}

pub fn verify_all(root: impl AsRef<Path>) -> Result<()> {
    let root = root.as_ref();
    crate::security::environment_boundary(root)?;
    crate::security::kaspa_pins(root, None, None)?;
    crate::security::advisories(root, 45)?;
    crate::security::action_pins(root)?;

    let first = documents(root)?;
    let second = documents(root)?;
    ensure!(first.len() == second.len(), "proof document count changed");
    for ((name_a, doc_a), (name_b, doc_b)) in first.iter().zip(second.iter()) {
        ensure!(name_a == name_b, "proof artifact ordering changed");
        ensure!(
            json_bytes(doc_a)? == json_bytes(doc_b)?,
            "proof is not reproducible: {name_a}"
        );
        ensure!(
            doc_a["schema_version"] == PROOF_SCHEMA_VERSION,
            "schema drift: {name_a}"
        );
        ensure!(doc_a["status"] == "PASS", "proof failed: {name_a}");
    }

    let written = write_documents(root, &first)?;
    println!(
        "proof-verify: PASS schema={PROOF_SCHEMA_VERSION} artifacts={} reproducible=yes",
        written.len()
    );
    for (name, digest) in written {
        println!("proof-artifact {name} sha256={digest}");
    }
    Ok(())
}
