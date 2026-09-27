use anyhow::{Context, Result, ensure};
use serde_json::{Value as JsonValue, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::Path;
use std::process::Command;
use toml::Value as TomlValue;

fn targets(policy: &TomlValue) -> Result<Vec<String>> {
    let mut targets = policy
        .get("targets")
        .and_then(TomlValue::as_array)
        .context("proof policy targets must be an array")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .context("proof target must be a string")
        })
        .collect::<Result<Vec<_>>>()?;
    targets.sort();
    targets.dedup();
    Ok(targets)
}

fn approval<'a>(
    policy: &'a TomlValue,
    name: &str,
    version: &str,
    source: &str,
) -> Result<&'a toml::map::Map<String, TomlValue>> {
    let entries = policy
        .get("native_approval")
        .and_then(TomlValue::as_array)
        .context("proof policy must define [[native_approval]] entries")?;
    entries
        .iter()
        .filter_map(TomlValue::as_table)
        .find(|entry| {
            entry.get("name").and_then(TomlValue::as_str) == Some(name)
                && entry.get("version").and_then(TomlValue::as_str) == Some(version)
                && entry.get("source").and_then(TomlValue::as_str) == Some(source)
        })
        .with_context(|| {
            format!("unapproved native/link dependency: {name}@{version} source={source}")
        })
}

fn field<'a>(table: &'a toml::map::Map<String, TomlValue>, key: &str) -> Result<&'a str> {
    table
        .get(key)
        .and_then(TomlValue::as_str)
        .with_context(|| format!("native approval missing {key}"))
}

fn cargo_lock(root: &Path) -> Result<(TomlValue, String)> {
    let bytes = fs::read(root.join("Cargo.lock"))?;
    let digest = crate::proof::sha256_hex(&bytes);
    let text = String::from_utf8(bytes).context("Cargo.lock is not UTF-8")?;
    Ok((text.parse().context("invalid Cargo.lock")?, digest))
}

fn lock_record(lock: &TomlValue, name: &str, version: &str, source: &str) -> Result<JsonValue> {
    let packages = lock
        .get("package")
        .and_then(TomlValue::as_array)
        .context("Cargo.lock package array missing")?;
    let package = packages
        .iter()
        .filter_map(TomlValue::as_table)
        .find(|item| {
            item.get("name").and_then(TomlValue::as_str) == Some(name)
                && item.get("version").and_then(TomlValue::as_str) == Some(version)
                && item.get("source").and_then(TomlValue::as_str) == Some(source)
        })
        .with_context(|| format!("lock entry missing for {name}@{version}"))?;
    Ok(json!({
        "locked_source": source,
        "package_checksum": package.get("checksum").and_then(TomlValue::as_str)
    }))
}

fn metadata(root: &Path, target: &str) -> Result<JsonValue> {
    let output = Command::new("cargo")
        .current_dir(root)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--filter-platform",
            target,
        ])
        .output()
        .context("failed to execute cargo metadata")?;
    ensure!(
        output.status.success(),
        "cargo metadata failed for {target}"
    );
    serde_json::from_slice(&output.stdout).context("invalid cargo metadata JSON")
}

fn production_edge(dep: &JsonValue) -> bool {
    let Some(kinds) = dep.get("dep_kinds").and_then(JsonValue::as_array) else {
        return true;
    };
    kinds.is_empty()
        || kinds.iter().any(|kind| {
            kind.get("kind").is_none_or(JsonValue::is_null)
                || kind.get("kind").and_then(JsonValue::as_str) == Some("build")
        })
}

fn entries_for_target(
    root: &Path,
    target: &str,
    policy: &TomlValue,
    lock: &TomlValue,
    lock_sha256: &str,
) -> Result<Vec<JsonValue>> {
    let data = metadata(root, target)?;
    let packages = data["packages"]
        .as_array()
        .context("metadata packages missing")?;
    let nodes = data["resolve"]["nodes"]
        .as_array()
        .context("metadata resolve nodes missing")?;
    let package_map = packages
        .iter()
        .map(|package| {
            let id = package["id"].as_str().context("package id missing")?;
            Ok((id.to_owned(), package))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let node_map = nodes
        .iter()
        .map(|node| {
            let id = node["id"].as_str().context("node id missing")?;
            Ok((id.to_owned(), node))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let root_id = packages
        .iter()
        .find(|package| {
            package["name"].as_str() == Some("kaspa-pulse") && package["source"].is_null()
        })
        .and_then(|package| package["id"].as_str())
        .context("kaspa-pulse root package missing")?
        .to_owned();

    let mut parent = BTreeMap::<String, Option<String>>::new();
    parent.insert(root_id.clone(), None);
    let mut queue = VecDeque::from([root_id.clone()]);
    while let Some(id) = queue.pop_front() {
        let node = node_map
            .get(&id)
            .with_context(|| format!("resolve node missing: {id}"))?;
        let deps = node["deps"].as_array().context("node deps missing")?;
        for dep in deps.iter().filter(|dep| production_edge(dep)) {
            let child = dep["pkg"]
                .as_str()
                .context("dependency package id missing")?;
            if !parent.contains_key(child) {
                parent.insert(child.to_owned(), Some(id.clone()));
                queue.push_back(child.to_owned());
            }
        }
    }

    let mut candidate_ids = parent
        .keys()
        .filter(|id| {
            let package = package_map.get(*id).expect("reachable package must exist");
            package["links"].as_str().is_some()
                || package["name"]
                    .as_str()
                    .is_some_and(|name| name.ends_with("-sys"))
        })
        .cloned()
        .collect::<Vec<_>>();
    candidate_ids.sort_by_key(|id| {
        let package = package_map.get(id).expect("candidate package must exist");
        (
            package["name"].as_str().unwrap_or_default().to_owned(),
            package["version"].as_str().unwrap_or_default().to_owned(),
            id.clone(),
        )
    });

    let mut entries = Vec::new();
    for id in candidate_ids {
        let package = package_map.get(&id).context("candidate package missing")?;
        let name = package["name"].as_str().context("candidate name missing")?;
        let version = package["version"]
            .as_str()
            .context("candidate version missing")?;
        let source = package["source"]
            .as_str()
            .context("candidate source missing")?;
        let approved = approval(policy, name, version, source)?;
        ensure!(
            field(approved, "approval")? == "APPROVED",
            "native candidate is not approved"
        );
        let mut path_ids = Vec::new();
        let mut current = Some(id.clone());
        while let Some(value) = current {
            path_ids.push(value.clone());
            current = parent.get(&value).cloned().flatten();
        }
        path_ids.reverse();
        let dependency_path = path_ids
            .iter()
            .map(|package_id| {
                let item = package_map.get(package_id).expect("path package exists");
                format!(
                    "{}@{}",
                    item["name"].as_str().unwrap_or_default(),
                    item["version"].as_str().unwrap_or_default()
                )
            })
            .collect::<Vec<_>>();
        let direct_or_transitive = if dependency_path.len() == 2 {
            "DIRECT"
        } else {
            "TRANSITIVE"
        };
        let native_code = approved
            .get("native_code")
            .and_then(TomlValue::as_bool)
            .context("native approval missing native_code")?;
        let mut locked = lock_record(lock, name, version, source)?;
        locked["cargo_lock_sha256"] = json!(lock_sha256);
        entries.push(json!({
            "package": name,
            "version": version,
            "source": source,
            "lock_identity": locked,
            "target": target,
            "role": field(approved, "role")?,
            "origin": field(approved, "origin")?,
            "classification": field(approved, "classification")?,
            "native_mechanism": field(approved, "native_mechanism")?,
            "native_code": native_code,
            "direct_or_transitive": direct_or_transitive,
            "dependency_path": dependency_path,
            "why_required": field(approved, "why_required")?,
            "policy_owner": field(approved, "policy_owner")?,
            "approval": field(approved, "approval")?,
            "validity_predicates": [
                "exact package name/version/source unchanged",
                "Cargo.lock identity unchanged",
                "target unchanged",
                "proof policy schema/approval unchanged"
            ]
        }));
    }
    Ok(entries)
}

pub fn document(root: &Path, policy: &TomlValue) -> Result<JsonValue> {
    let (lock, lock_sha256) = cargo_lock(root)?;
    let target_list = targets(policy)?;
    let mut entries = Vec::new();
    for target in &target_list {
        entries.extend(entries_for_target(
            root,
            target,
            policy,
            &lock,
            &lock_sha256,
        )?);
    }
    entries.sort_by_key(|entry| {
        format!(
            "{}|{}|{}|{}",
            entry["target"].as_str().unwrap_or_default(),
            entry["package"].as_str().unwrap_or_default(),
            entry["version"].as_str().unwrap_or_default(),
            entry["source"].as_str().unwrap_or_default()
        )
    });
    let seen = entries
        .iter()
        .map(|entry| {
            format!(
                "{}|{}|{}",
                entry["package"].as_str().unwrap_or_default(),
                entry["version"].as_str().unwrap_or_default(),
                entry["source"].as_str().unwrap_or_default()
            )
        })
        .collect::<BTreeSet<_>>();
    let approvals = policy
        .get("native_approval")
        .and_then(TomlValue::as_array)
        .context("native approval list missing")?
        .iter()
        .filter_map(TomlValue::as_table)
        .map(|entry| {
            format!(
                "{}|{}|{}",
                entry
                    .get("name")
                    .and_then(TomlValue::as_str)
                    .unwrap_or_default(),
                entry
                    .get("version")
                    .and_then(TomlValue::as_str)
                    .unwrap_or_default(),
                entry
                    .get("source")
                    .and_then(TomlValue::as_str)
                    .unwrap_or_default()
            )
        })
        .collect::<BTreeSet<_>>();
    ensure!(
        seen == approvals,
        "native approval set must exactly match discovered candidates"
    );
    let native_code_entries = entries
        .iter()
        .filter(|entry| entry["native_code"] == true)
        .count();
    Ok(json!({
        "schema_version": crate::proof::PROOF_SCHEMA_VERSION,
        "proof_type": "transitive-native-dependency-inventory",
        "status": "PASS",
        "discovery_rule": "production-reachable Cargo packages with links metadata or -sys package names; dev-only edges excluded",
        "cargo_lock_sha256": lock_sha256,
        "targets": target_list,
        "summary": {
            "classified_entries": entries.len(),
            "unique_approved_candidates": seen.len(),
            "native_code_entries": native_code_entries,
            "unapproved_native_dependencies": 0
        },
        "dependencies": entries
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_native_dependency_fails_closed() {
        let policy: TomlValue = r#"
            [[native_approval]]
            name = "known-sys"
            version = "1.0.0"
            source = "registry+https://github.com/rust-lang/crates.io-index"
        "#
        .parse()
        .unwrap();
        assert!(
            approval(
                &policy,
                "future-native-sys",
                "9.9.9",
                "registry+https://github.com/rust-lang/crates.io-index"
            )
            .is_err()
        );
    }
}
