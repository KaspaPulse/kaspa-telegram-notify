use anyhow::{Context, Result, ensure};
use regex::Regex;
use serde_json::{Value as JsonValue, json};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::Path;
use std::process::Command;
use toml::Value as TomlValue;

#[derive(Debug, Clone)]
struct BuildScriptEvidence {
    sha256: String,
    signals: Vec<String>,
}

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
            format!("unapproved native/link/build dependency: {name}@{version} source={source}")
        })
}

fn field<'a>(table: &'a toml::map::Map<String, TomlValue>, key: &str) -> Result<&'a str> {
    table
        .get(key)
        .and_then(TomlValue::as_str)
        .with_context(|| format!("native approval missing {key}"))
}

fn bool_field(table: &toml::map::Map<String, TomlValue>, key: &str) -> Result<bool> {
    table
        .get(key)
        .and_then(TomlValue::as_bool)
        .with_context(|| format!("native approval missing {key}"))
}
fn string_array(
    table: &toml::map::Map<String, TomlValue>,
    key: &str,
    required: bool,
) -> Result<Vec<String>> {
    let Some(value) = table.get(key) else {
        ensure!(!required, "native approval missing {key}");
        return Ok(Vec::new());
    };
    let mut values = value
        .as_array()
        .with_context(|| format!("native approval {key} must be an array"))?
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_owned)
                .with_context(|| format!("native approval {key} entries must be strings"))
        })
        .collect::<Result<Vec<_>>>()?;
    values.sort();
    values.dedup();
    Ok(values)
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

fn custom_build_path(package: &JsonValue) -> Option<&str> {
    package
        .get("targets")?
        .as_array()?
        .iter()
        .find_map(|target| {
            let is_custom_build = target
                .get("kind")?
                .as_array()?
                .iter()
                .any(|kind| kind.as_str() == Some("custom-build"));
            is_custom_build
                .then(|| target.get("src_path")?.as_str())
                .flatten()
        })
}
fn native_build_signals(text: &str) -> Result<Vec<String>> {
    let compact = text
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>();
    let native_literal = Regex::new(r#"(?i)["'][^"'\n]+\.(?:c|cc|cpp|cxx|s|asm|a|o)["']"#)?;
    let command_literal = Regex::new(r#"Command\s*::\s*new\s*\(\s*"([^"]+)""#)?;
    let mut signals = BTreeSet::new();
    for (needle, signal) in [
        ("cc::Build", "BUILD_SCRIPT_CC"),
        ("cmake::", "BUILD_SCRIPT_CMAKE"),
        ("pkg_config::", "BUILD_SCRIPT_PKG_CONFIG"),
        ("vcpkg::", "BUILD_SCRIPT_VCPKG"),
        ("bindgen::", "BUILD_SCRIPT_BINDGEN"),
    ] {
        if compact.contains(needle) {
            signals.insert(signal.to_owned());
        }
    }
    for captures in command_literal.captures_iter(text) {
        let program = captures
            .get(1)
            .map(|value| value.as_str())
            .unwrap_or_default();
        let tool = program
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(program)
            .to_ascii_lowercase();
        let native_tool = matches!(
            tool.as_str(),
            "cc" | "gcc"
                | "clang"
                | "clang++"
                | "g++"
                | "c++"
                | "nasm"
                | "yasm"
                | "ar"
                | "ld"
                | "make"
                | "cmake"
                | "meson"
                | "ninja"
                | "pkg-config"
                | "sh"
                | "bash"
                | "zsh"
                | "cmd"
                | "powershell"
                | "pwsh"
        ) || tool.starts_with("gcc-")
            || tool.starts_with("clang-");
        if native_tool {
            signals.insert("BUILD_SCRIPT_NATIVE_TOOL_COMMAND".to_owned());
        }
    }
    if text.contains("rustc-link-lib") {
        signals.insert("BUILD_SCRIPT_RUSTC_LINK_LIB".to_owned());
    }
    if text.contains("rustc-link-search") {
        signals.insert("BUILD_SCRIPT_RUSTC_LINK_SEARCH".to_owned());
    }
    if native_literal.is_match(text) {
        signals.insert("BUILD_SCRIPT_NATIVE_SOURCE_LITERAL".to_owned());
    }
    Ok(signals.into_iter().collect())
}

fn build_script_evidence(package: &JsonValue) -> Result<Option<BuildScriptEvidence>> {
    let Some(path) = custom_build_path(package) else {
        return Ok(None);
    };
    let bytes =
        fs::read(path).with_context(|| format!("failed to read custom build script: {path}"))?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(Some(BuildScriptEvidence {
        sha256: crate::proof::sha256_hex(&bytes),
        signals: native_build_signals(&text)?,
    }))
}
fn dependency_path(
    id: &str,
    parent: &BTreeMap<String, Option<String>>,
    package_map: &BTreeMap<String, &JsonValue>,
) -> Vec<String> {
    let mut path_ids = Vec::new();
    let mut current = Some(id.to_owned());
    while let Some(value) = current {
        path_ids.push(value.clone());
        current = parent.get(&value).cloned().flatten();
    }
    path_ids.reverse();
    path_ids
        .iter()
        .map(|package_id| {
            let item = package_map.get(package_id).expect("path package exists");
            format!(
                "{}@{}",
                item["name"].as_str().unwrap_or_default(),
                item["version"].as_str().unwrap_or_default()
            )
        })
        .collect()
}

fn source_identity(package: &JsonValue) -> String {
    package["source"].as_str().unwrap_or("workspace").to_owned()
}

fn candidate_signals(package: &JsonValue, build: Option<&BuildScriptEvidence>) -> Vec<String> {
    let mut signals = BTreeSet::new();
    if package["links"].as_str().is_some() {
        signals.insert("CARGO_LINKS".to_owned());
    }
    if package["name"]
        .as_str()
        .is_some_and(|name| name.ends_with("-sys"))
    {
        signals.insert("SYS_SUFFIX".to_owned());
    }
    if let Some(build) = build {
        signals.extend(build.signals.iter().cloned());
    }
    signals.into_iter().collect()
}
fn entries_for_target(
    root: &Path,
    target: &str,
    policy: &TomlValue,
    lock: &TomlValue,
    lock_sha256: &str,
) -> Result<(Vec<JsonValue>, Vec<JsonValue>)> {
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
    let mut queue = VecDeque::from([root_id]);
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

    let mut build_map = BTreeMap::<String, BuildScriptEvidence>::new();
    let mut build_coverage = Vec::new();
    for id in parent.keys() {
        let package = package_map.get(id).context("reachable package missing")?;
        let Some(build) = build_script_evidence(package)? else {
            continue;
        };
        let name = package["name"]
            .as_str()
            .context("build package name missing")?;
        let version = package["version"]
            .as_str()
            .context("build package version missing")?;
        let source = source_identity(package);
        let signals = build.signals.clone();
        build_coverage.push(json!({
            "target": target,
            "package": name,
            "version": version,
            "source": source,
            "build_script_sha256": build.sha256,
            "native_capability_signals": signals,
            "classification": if signals.is_empty() {
                "NO_NATIVE_CAPABILITY_SIGNAL_DETECTED"
            } else {
                "NATIVE_CAPABILITY_SIGNAL_DETECTED"
            },
            "dependency_path": dependency_path(id, &parent, &package_map)
        }));
        build_map.insert(id.clone(), build);
    }
    let mut candidate_ids = parent
        .keys()
        .filter(|id| {
            let package = package_map.get(*id).expect("reachable package must exist");
            !candidate_signals(package, build_map.get(*id)).is_empty()
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

    let supported_targets = targets(policy)?;
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
        let native_capability = bool_field(approved, "native_code")?;
        let active_targets = string_array(approved, "active_native_targets", true)?;
        ensure!(
            active_targets
                .iter()
                .all(|value| supported_targets.contains(value)),
            "native approval has unsupported active target: {name}@{version}"
        );
        ensure!(
            native_capability || active_targets.is_empty(),
            "non-native approval cannot declare active native targets: {name}@{version}"
        );
        let resolved_features = node_map
            .get(&id)
            .and_then(|node| node["features"].as_array())
            .into_iter()
            .flatten()
            .filter_map(JsonValue::as_str)
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        let required_features = string_array(approved, "required_features", false)?;
        ensure!(
            required_features
                .iter()
                .all(|feature| resolved_features.contains(feature)),
            "required native-proof feature missing for {name}@{version}"
        );
        let native_active_for_target = active_targets.iter().any(|value| value == target);
        ensure!(
            !native_active_for_target || native_capability,
            "active native target requires native_code=true"
        );

        let path = dependency_path(&id, &parent, &package_map);
        let direct_or_transitive = if path.len() == 2 {
            "DIRECT"
        } else {
            "TRANSITIVE"
        };
        let mut locked = lock_record(lock, name, version, source)?;
        locked["cargo_lock_sha256"] = json!(lock_sha256);
        let build = build_map.get(&id);
        let discovery_signals = candidate_signals(package, build);
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
            "native_code": native_capability,
            "native_active_for_target": native_active_for_target,
            "active_native_targets": active_targets,
            "resolved_features": resolved_features,
            "required_features": required_features,
            "discovery_signals": discovery_signals,
            "build_script_sha256": build.map(|value| value.sha256.clone()),
            "direct_or_transitive": direct_or_transitive,
            "dependency_path": path,
            "why_required": field(approved, "why_required")?,
            "policy_owner": field(approved, "policy_owner")?,
            "approval": field(approved, "approval")?,
            "validity_predicates": [
                "exact package name/version/source unchanged",
                "Cargo.lock identity unchanged",
                "target and resolved feature set unchanged",
                "custom build script digest/signals unchanged when present",
                "proof policy schema/approval unchanged"
            ]
        }));
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
    build_coverage.sort_by_key(|entry| {
        format!(
            "{}|{}|{}|{}",
            entry["target"].as_str().unwrap_or_default(),
            entry["package"].as_str().unwrap_or_default(),
            entry["version"].as_str().unwrap_or_default(),
            entry["source"].as_str().unwrap_or_default()
        )
    });
    Ok((entries, build_coverage))
}
pub fn document(root: &Path, policy: &TomlValue) -> Result<JsonValue> {
    let (lock, lock_sha256) = cargo_lock(root)?;
    let target_list = targets(policy)?;
    let mut entries = Vec::new();
    let mut build_scripts = Vec::new();
    for target in &target_list {
        let (target_entries, target_build_scripts) =
            entries_for_target(root, target, policy, &lock, &lock_sha256)?;
        entries.extend(target_entries);
        build_scripts.extend(target_build_scripts);
    }

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

    let native_capability_entries = entries
        .iter()
        .filter(|entry| entry["native_code"] == true)
        .count();
    let active_native_entries = entries
        .iter()
        .filter(|entry| entry["native_active_for_target"] == true)
        .count();
    let signaled_build_scripts = build_scripts
        .iter()
        .filter(|entry| {
            entry["native_capability_signals"]
                .as_array()
                .is_some_and(|signals| !signals.is_empty())
        })
        .count();

    Ok(json!({
        "schema_version": crate::proof::PROOF_SCHEMA_VERSION,
        "proof_type": "transitive-native-dependency-inventory",
        "status": "PASS",
        "discovery_rule": "all production-reachable custom build scripts are recorded; explicit native policy is required for Cargo links, -sys names, or build scripts with compiler/link/process/native-source capability signals; dev-only edges excluded",
        "cargo_lock_sha256": lock_sha256,
        "targets": target_list,
        "summary": {
            "classified_entries": entries.len(),
            "unique_approved_candidates": seen.len(),
            "native_capability_entries": native_capability_entries,
            "active_native_entries": active_native_entries,
            "production_reachable_custom_build_scripts": build_scripts.len(),
            "native_capability_build_scripts": signaled_build_scripts,
            "unapproved_native_dependencies": 0
        },
        "dependencies": entries,
        "custom_build_scripts": build_scripts
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

    #[test]
    fn native_build_signals_detect_compiler_link_and_command_paths() {
        let text = r#"
            let mut c = cc::Build::new();
            c.file("src/native.S");
            println!("cargo:rustc-link-lib=static=native");
            let _ = std::process::Command::new("clang");
        "#;
        let signals = native_build_signals(text).unwrap();
        assert!(signals.contains(&"BUILD_SCRIPT_CC".to_owned()));
        assert!(signals.contains(&"BUILD_SCRIPT_NATIVE_SOURCE_LITERAL".to_owned()));
        assert!(signals.contains(&"BUILD_SCRIPT_RUSTC_LINK_LIB".to_owned()));
        assert!(signals.contains(&"BUILD_SCRIPT_NATIVE_TOOL_COMMAND".to_owned()));
    }
    #[test]
    fn pure_build_logic_has_no_native_capability_signal() {
        let text = r#"
            fn main() {
                println!("cargo:rerun-if-changed=src/lib.rs");
                println!("cargo:rustc-cfg=feature_probe");
            }
        "#;
        assert!(native_build_signals(text).unwrap().is_empty());
    }

    #[test]
    fn rustc_and_non_native_command_probes_do_not_become_native_candidates() {
        let text = r#"
            let mut rustc = std::process::Command::new(rustc_path);
            let _ = std::process::Command::new("git");
            let _ = std::process::Command::new("freebsd-version");
        "#;
        assert!(native_build_signals(text).unwrap().is_empty());
    }

    #[test]
    fn target_activation_is_policy_explicit() {
        let policy: TomlValue = r#"
            active_native_targets = ["x86_64-unknown-linux-gnu"]
        "#
        .parse()
        .unwrap();
        let table = policy.as_table().unwrap();
        assert_eq!(
            string_array(table, "active_native_targets", true).unwrap(),
            vec!["x86_64-unknown-linux-gnu".to_owned()]
        );
    }
}
