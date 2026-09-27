use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::fs;
use std::path::Path;
use uuid::Uuid;

pub fn finalize(path: &Path, version: &str, target: &str) -> Result<()> {
    let repository = std::env::var("GITHUB_REPOSITORY")
        .context("cyclonedx-finalize: GITHUB_REPOSITORY is required")?;
    let commit_sha =
        std::env::var("GITHUB_SHA").context("cyclonedx-finalize: GITHUB_SHA is required")?;
    finalize_with_identity(path, version, target, &repository, &commit_sha)
}

fn finalize_with_identity(
    path: &Path,
    version: &str,
    target: &str,
    repository: &str,
    commit_sha: &str,
) -> Result<()> {
    ensure!(
        path.is_file(),
        "cyclonedx-finalize: SBOM is missing: {}",
        path.display()
    );
    ensure!(
        fs::metadata(path)?.len() > 0,
        "cyclonedx-finalize: SBOM is empty: {}",
        path.display()
    );

    let repository = repository.trim();
    ensure!(
        !repository.is_empty(),
        "cyclonedx-finalize: GITHUB_REPOSITORY is required"
    );
    let commit_sha = commit_sha.trim().to_ascii_lowercase();
    ensure!(
        commit_sha.len() == 40
            && commit_sha
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "cyclonedx-finalize: GITHUB_SHA must be a full lowercase 40-character Git commit SHA"
    );

    let raw = fs::read_to_string(path)
        .with_context(|| format!("cyclonedx-finalize: cannot read {}", path.display()))?;
    let mut document: Value = serde_json::from_str(&raw)
        .with_context(|| format!("cyclonedx-finalize: cannot parse {}", path.display()))?;
    let object = document
        .as_object_mut()
        .context("cyclonedx-finalize: CycloneDX document must be a JSON object")?;
    ensure!(
        object.get("bomFormat").and_then(Value::as_str) == Some("CycloneDX"),
        "cyclonedx-finalize: bomFormat must be exactly 'CycloneDX'"
    );
    let spec_version = object
        .get("specVersion")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|value| !value.trim().is_empty())
        .context("cyclonedx-finalize: specVersion is required")?;

    let seed = format!(
        "https://github.com/{repository}@{commit_sha}#kaspa-pulse:{version}:{target}:cyclonedx-{spec_version}"
    );
    let serial_uuid = Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes());
    let serial = format!("urn:uuid:{serial_uuid}");
    object.insert("serialNumber".to_owned(), Value::String(serial.clone()));

    let encoded = serde_json::to_string(&document)? + "\n";
    fs::write(path, encoded)
        .with_context(|| format!("cyclonedx-finalize: cannot write {}", path.display()))?;

    let normalized: Value = serde_json::from_str(&fs::read_to_string(path)?)?;
    let normalized_serial = normalized
        .get("serialNumber")
        .and_then(Value::as_str)
        .context("cyclonedx-finalize: serialNumber normalization failed")?;
    let uuid_text = normalized_serial
        .strip_prefix("urn:uuid:")
        .context("cyclonedx-finalize: serialNumber must use urn:uuid")?;
    Uuid::parse_str(uuid_text).context("cyclonedx-finalize: invalid serialNumber UUID")?;

    println!(
        "cyclonedx-finalize: PASS (specVersion={spec_version}, serialNumber=generated, sha={commit_sha})"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn deterministic_serial_matches_uuid_v5_contract() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bom.json");
        fs::write(
            &path,
            r#"{"bomFormat":"CycloneDX","specVersion":"1.5","components":[]}"#,
        )
        .unwrap();

        finalize_with_identity(
            &path,
            "1.2.10",
            "x86_64-unknown-linux-gnu",
            "KaspaPulse/kaspa-telegram-notify",
            "0123456789abcdef0123456789abcdef01234567",
        )
        .unwrap();
        let first = fs::read_to_string(&path).unwrap();

        fs::write(
            &path,
            r#"{"bomFormat":"CycloneDX","specVersion":"1.5","components":[]}"#,
        )
        .unwrap();
        finalize_with_identity(
            &path,
            "1.2.10",
            "x86_64-unknown-linux-gnu",
            "KaspaPulse/kaspa-telegram-notify",
            "0123456789abcdef0123456789abcdef01234567",
        )
        .unwrap();
        let second = fs::read_to_string(&path).unwrap();

        assert_eq!(first, second);
        let value: Value = serde_json::from_str(&first).unwrap();
        assert!(
            value["serialNumber"]
                .as_str()
                .unwrap()
                .starts_with("urn:uuid:")
        );
    }
}
