use super::*;
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    process::Command,
};

const IMPACT_SCHEMA_VERSION: &str = "opqual-impact/v1";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PathClass {
    Critical,
    ProvenIrrelevant,
    Unknown,
}

#[derive(Clone, Debug)]
struct Policy {
    critical_exact: Vec<Vec<u8>>,
    critical_prefixes: Vec<Vec<u8>>,
    irrelevant_exact: Vec<Vec<u8>>,
    irrelevant_prefixes: Vec<Vec<u8>>,
}

type ManifestLoad = (
    Value,
    BTreeMap<String, String>,
    BTreeMap<String, Policy>,
    String,
);

pub(super) fn execute(
    base_sha: &str,
    tested_sha: &str,
    baseline_proof: Option<&Path>,
    output: &Path,
) -> Result<()> {
    let repo = std::env::current_dir()?.canonicalize()?;
    ensure!(
        repo.join("Cargo.toml").is_file(),
        "run impact classification from repository root"
    );

    let actual_head = git_text(&repo, &["rev-parse", "HEAD"])?;
    ensure!(
        actual_head.trim() == tested_sha,
        "TESTED_SHA {} is not checked out HEAD {}",
        tested_sha,
        actual_head.trim()
    );
    let status = Command::new("git")
        .current_dir(&repo)
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
        .output()
        .context("git status for impact proof")?;
    ensure!(
        status.status.success(),
        "git status failed during impact proof"
    );
    ensure!(
        status.stdout.is_empty(),
        "impact proof requires a clean TESTED_SHA checkout"
    );

    let base_tree = git_tree(&repo, base_sha)?;
    let tested_tree = git_tree(&repo, tested_sha)?;
    let manifest_path = repo.join("opqual/scenario-surfaces-v1.json");
    let (manifest, scenario_policy, policies, manifest_sha256) =
        load_manifest(&repo, &manifest_path)?;

    let matrix_ids = matrix_ids(&repo.join("opqual/scenario-map.csv"))?;
    let manifest_ids = scenario_policy.keys().cloned().collect::<BTreeSet<_>>();
    ensure!(
        matrix_ids == manifest_ids,
        "surface manifest scenario coverage differs from scenario-map.csv"
    );

    let changed = changed_paths(&repo, base_sha, tested_sha)?;
    let baseline = validate_baseline(&repo, baseline_proof, base_sha, &base_tree, &matrix_ids);

    let mut invalidated = BTreeSet::new();
    let mut reusable = BTreeSet::new();
    let mut unknown_paths = BTreeSet::<Vec<u8>>::new();

    for scenario_id in &matrix_ids {
        let policy_name = scenario_policy
            .get(scenario_id)
            .with_context(|| format!("policy missing for scenario {scenario_id}"))?;
        let policy = policies
            .get(policy_name)
            .with_context(|| format!("unknown policy {policy_name} for {scenario_id}"))?;
        let mut invalidate = false;
        for path in &changed {
            match classify_path(policy, path) {
                PathClass::ProvenIrrelevant => {}
                PathClass::Critical => invalidate = true,
                PathClass::Unknown => {
                    invalidate = true;
                    unknown_paths.insert(path.clone());
                }
            }
        }
        if invalidate {
            invalidated.insert(scenario_id.clone());
        } else {
            reusable.insert(scenario_id.clone());
        }
    }

    let baseline_ok = baseline
        .as_ref()
        .is_ok_and(|validated| validated.eligible_for_reuse);
    let classification = if invalidated.is_empty() && baseline_ok {
        "REUSE_ALL"
    } else {
        "FULL_HERMETIC_E2E"
    };

    let changed_json = changed
        .iter()
        .map(|path| {
            json!({
                "path_bytes_base64": base64_encode(path),
                "display_path": std::str::from_utf8(path).ok(),
                "overall_classification": overall_path_class(&policies, path)
            })
        })
        .collect::<Vec<_>>();

    let (baseline_json, baseline_reason) = match &baseline {
        Ok(value) => (
            json!({
                "available": true,
                "eligible_for_reuse": value.eligible_for_reuse,
                "canonical_sha256": value.canonical_sha256,
                "tested_sha": value.tested_sha,
                "tested_tree": value.tested_tree
            }),
            value.reason.clone(),
        ),
        Err(error) => (
            json!({"available":baseline_proof.is_some(),"eligible_for_reuse":false}),
            format!("{error:#}"),
        ),
    };

    let proof = json!({
        "schema_version": IMPACT_SCHEMA_VERSION,
        "result": "PASS",
        "classification": classification,
        "policy": {
            "under_invalidation": "FORBIDDEN",
            "over_invalidation": "ACCEPTABLE",
            "unknown_impact": "FULL_HERMETIC_E2E",
            "path_identity": "RAW_GIT_PATH_BYTES_BASE64"
        },
        "identity": {
            "base_sha": base_sha,
            "base_tree": base_tree,
            "tested_sha": tested_sha,
            "tested_tree": tested_tree
        },
        "surface_manifest": {
            "path": "opqual/scenario-surfaces-v1.json",
            "sha256": manifest_sha256,
            "schema_version": manifest.get("schema_version")
        },
        "baseline_proof": baseline_json,
        "baseline_reason": baseline_reason,
        "changed_paths": changed_json,
        "unknown_paths": unknown_paths.iter().map(|path| base64_encode(path)).collect::<Vec<_>>(),
        "invalidated_scenarios": invalidated.iter().collect::<Vec<_>>(),
        "reusable_scenarios": reusable.iter().collect::<Vec<_>>(),
        "counts": {
            "scenario_total": matrix_ids.len(),
            "invalidated": invalidated.len(),
            "reusable": reusable.len(),
            "changed_paths": changed.len(),
            "unknown_paths": unknown_paths.len()
        }
    });

    let canonical = canonical_json(&proof)?;
    let digest = sha256_bytes(&canonical);
    let output = if output.is_absolute() {
        output.to_path_buf()
    } else {
        repo.join(output)
    };
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    atomic_bytes(&output, &canonical)?;
    let sha_path = output.with_extension("sha256");
    atomic_bytes(&sha_path, format!("{digest}\n").as_bytes())?;

    println!("IMPACT_CLASSIFICATION={classification}");
    println!("IMPACT_PROOF_SHA256={digest}");
    println!("IMPACT_PROOF_PATH={}", output.display());
    println!("INVALIDATED_SCENARIOS={}", invalidated.len());
    println!("REUSABLE_SCENARIOS={}", reusable.len());
    println!("UNKNOWN_PATHS={}", unknown_paths.len());
    Ok(())
}

#[derive(Clone, Debug)]
struct ValidatedBaseline {
    eligible_for_reuse: bool,
    reason: String,
    canonical_sha256: String,
    tested_sha: String,
    tested_tree: String,
}

fn validate_baseline(
    repo: &Path,
    baseline_proof: Option<&Path>,
    base_sha: &str,
    base_tree: &str,
    matrix_ids: &BTreeSet<String>,
) -> Result<ValidatedBaseline> {
    let path = baseline_proof.context("baseline canonical proof not supplied")?;
    let bytes =
        fs::read(path).with_context(|| format!("read baseline proof {}", path.display()))?;
    let value: Value = serde_json::from_slice(&bytes).context("parse baseline canonical proof")?;
    ensure!(
        canonical_json(&value)? == bytes,
        "baseline proof is not RFC8785/JCS canonical bytes"
    );
    let digest = sha256_bytes(&bytes);
    let sidecar = path.with_extension("sha256");
    let expected = fs::read_to_string(&sidecar)
        .with_context(|| format!("read baseline proof digest {}", sidecar.display()))?;
    ensure!(
        expected.trim() == digest,
        "baseline proof SHA256 sidecar does not match canonical proof"
    );

    let tested = value
        .pointer("/identity/tested_sha")
        .and_then(Value::as_str);
    let tree = value
        .pointer("/identity/tested_tree")
        .and_then(Value::as_str);
    ensure!(
        tested == Some(base_sha),
        "baseline proof is not bound to BASE_SHA"
    );
    ensure!(
        tree == Some(base_tree),
        "baseline proof is not bound to BASE_TREE"
    );
    ensure!(
        value.get("result").and_then(Value::as_str) == Some("PASS")
            && value.get("audit_status").and_then(Value::as_str) == Some("COMPLETE")
            && value.get("cleanup").and_then(Value::as_str) == Some("PASS"),
        "baseline proof is not a complete successful audit"
    );
    let summary = value
        .get("scenario_summary")
        .and_then(Value::as_object)
        .context("baseline scenario_summary missing")?;
    for key in ["blocked", "verified_fail", "not_tested"] {
        ensure!(
            summary.get(key).and_then(Value::as_u64) == Some(0),
            "baseline {key} is not zero"
        );
    }
    ensure!(
        summary
            .get("full_bot_journeys_completed")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            >= 150,
        "baseline full journey threshold is not met"
    );

    let scenario_results = value
        .get("scenario_canonical_results")
        .and_then(Value::as_object)
        .context("baseline scenario_canonical_results missing")?;
    let proof_ids = scenario_results.keys().cloned().collect::<BTreeSet<_>>();
    ensure!(
        &proof_ids == matrix_ids,
        "baseline proof scenario set differs from current manifest/matrix"
    );

    let current_base_tree = git_tree(repo, base_sha)?;
    ensure!(
        current_base_tree == base_tree,
        "base tree changed during baseline validation"
    );

    Ok(ValidatedBaseline {
        eligible_for_reuse: true,
        reason:
            "baseline canonical proof is exact-base, complete, canonical, and covers every scenario"
                .into(),
        canonical_sha256: digest,
        tested_sha: base_sha.into(),
        tested_tree: base_tree.into(),
    })
}

fn load_manifest(repo: &Path, path: &Path) -> Result<ManifestLoad> {
    let bytes = fs::read(path)?;
    let manifest: Value = serde_json::from_slice(&bytes)?;
    ensure!(
        manifest.get("schema_version").and_then(Value::as_str) == Some("scenario-surfaces/v1"),
        "unsupported scenario surface manifest schema"
    );
    let mut scenarios = BTreeMap::new();
    for (id, policy) in manifest
        .get("scenarios")
        .and_then(Value::as_object)
        .context("surface manifest scenarios missing")?
    {
        let policy = policy
            .as_str()
            .with_context(|| format!("scenario policy for {id} must be a string"))?;
        ensure!(
            scenarios.insert(id.clone(), policy.to_owned()).is_none(),
            "duplicate scenario {id}"
        );
    }
    let mut policies = BTreeMap::new();
    for (name, value) in manifest
        .get("policies")
        .and_then(Value::as_object)
        .context("surface manifest policies missing")?
    {
        policies.insert(name.clone(), parse_policy(value)?);
    }
    for policy in scenarios.values() {
        ensure!(
            policies.contains_key(policy),
            "scenario references unknown policy {policy}"
        );
    }
    let digest = sha256_bytes(&bytes);
    ensure!(
        path.starts_with(repo),
        "surface manifest must be repository-local"
    );
    Ok((manifest, scenarios, policies, digest))
}

fn parse_policy(value: &Value) -> Result<Policy> {
    let list = |key: &str| -> Result<Vec<Vec<u8>>> {
        value
            .get(key)
            .and_then(Value::as_array)
            .with_context(|| format!("policy {key} missing"))?
            .iter()
            .map(|item| {
                item.as_str()
                    .with_context(|| format!("policy {key} entry must be a string"))
                    .map(|text| text.as_bytes().to_vec())
            })
            .collect::<Result<Vec<_>>>()
    };
    Ok(Policy {
        critical_exact: list("critical_exact")?,
        critical_prefixes: list("critical_prefixes")?,
        irrelevant_exact: list("proven_irrelevant_exact")?,
        irrelevant_prefixes: list("proven_irrelevant_prefixes")?,
    })
}

fn classify_path(policy: &Policy, path: &[u8]) -> PathClass {
    if policy
        .critical_exact
        .iter()
        .any(|item| item.as_slice() == path)
        || policy
            .critical_prefixes
            .iter()
            .any(|prefix| path.starts_with(prefix))
    {
        return PathClass::Critical;
    }
    if policy
        .irrelevant_exact
        .iter()
        .any(|item| item.as_slice() == path)
        || policy
            .irrelevant_prefixes
            .iter()
            .any(|prefix| path.starts_with(prefix))
    {
        return PathClass::ProvenIrrelevant;
    }
    PathClass::Unknown
}

fn overall_path_class(policies: &BTreeMap<String, Policy>, path: &[u8]) -> &'static str {
    let mut irrelevant = true;
    for policy in policies.values() {
        match classify_path(policy, path) {
            PathClass::Critical => return "CRITICAL",
            PathClass::Unknown => irrelevant = false,
            PathClass::ProvenIrrelevant => {}
        }
    }
    if irrelevant {
        "PROVEN_IRRELEVANT"
    } else {
        "UNKNOWN"
    }
}

fn matrix_ids(path: &Path) -> Result<BTreeSet<String>> {
    let text = fs::read_to_string(path)?;
    let mut lines = text.lines();
    let header = lines
        .next()
        .context("scenario matrix header missing")?
        .trim_start_matches('\u{feff}');
    ensure!(
        header.split(',').next() == Some("id"),
        "scenario matrix id must be first column"
    );
    let mut ids = BTreeSet::new();
    for line in lines.filter(|line| !line.trim().is_empty()) {
        let id = line
            .split(',')
            .next()
            .context("scenario matrix row missing id")?;
        ensure!(!id.is_empty(), "scenario matrix row has empty id");
        ensure!(
            ids.insert(id.to_owned()),
            "duplicate scenario matrix id {id}"
        );
    }
    Ok(ids)
}

fn changed_paths(repo: &Path, base_sha: &str, tested_sha: &str) -> Result<Vec<Vec<u8>>> {
    let output = Command::new("git")
        .current_dir(repo)
        .args([
            "diff",
            "--name-status",
            "-z",
            "--find-renames",
            base_sha,
            tested_sha,
            "--",
        ])
        .output()
        .context("git diff for impact classification")?;
    ensure!(
        output.status.success(),
        "git diff failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    parse_name_status_z(&output.stdout)
}

fn parse_name_status_z(bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
    let parts = bytes
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let mut index = 0usize;
    let mut paths = BTreeSet::new();
    while index < parts.len() {
        let status = std::str::from_utf8(parts[index]).context("git diff status is not UTF-8")?;
        index += 1;
        let path_count = if status.starts_with('R') || status.starts_with('C') {
            2
        } else {
            1
        };
        ensure!(
            index + path_count <= parts.len(),
            "truncated git diff --name-status -z output"
        );
        for _ in 0..path_count {
            paths.insert(parts[index].to_vec());
            index += 1;
        }
    }
    Ok(paths.into_iter().collect())
}

fn git_tree(repo: &Path, sha: &str) -> Result<String> {
    let value = git_text(repo, &["rev-parse", &format!("{sha}^{{tree}}")])?;
    Ok(value.trim().to_owned())
}

fn git_text(repo: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git").current_dir(repo).args(args).output()?;
    ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?)
}

fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        out.push(TABLE[(a >> 2) as usize] as char);
        out.push(TABLE[(((a & 0x03) << 4) | (b >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(((b & 0x0f) << 2) | (c >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[(c & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> Policy {
        Policy {
            critical_exact: vec![b"Cargo.lock".to_vec()],
            critical_prefixes: vec![b"src/".to_vec()],
            irrelevant_exact: vec![b"README.md".to_vec()],
            irrelevant_prefixes: vec![b"docs/".to_vec()],
        }
    }

    #[test]
    fn classifier_is_fail_closed() {
        let policy = policy();
        assert_eq!(classify_path(&policy, b"src/main.rs"), PathClass::Critical);
        assert_eq!(classify_path(&policy, b"Cargo.lock"), PathClass::Critical);
        assert_eq!(
            classify_path(&policy, b"docs/design.md"),
            PathClass::ProvenIrrelevant
        );
        assert_eq!(
            classify_path(&policy, b"README.md"),
            PathClass::ProvenIrrelevant
        );
        assert_eq!(classify_path(&policy, b"mystery.bin"), PathClass::Unknown);
    }

    #[test]
    fn raw_path_base64_handles_non_utf8_bytes() {
        assert_eq!(base64_encode(&[0xff, 0x00, 0x80]), "/wCA");
        assert_eq!(base64_encode(b"src/main.rs"), "c3JjL21haW4ucnM=");
    }

    #[test]
    fn name_status_parser_preserves_both_rename_paths() {
        let raw = b"M\0src/main.rs\0R100\0old\xff.rs\0new\xff.rs\0";
        let paths = parse_name_status_z(raw).unwrap();
        assert!(paths.contains(&b"src/main.rs".to_vec()));
        assert!(paths.contains(&b"old\xff.rs".to_vec()));
        assert!(paths.contains(&b"new\xff.rs".to_vec()));
    }

    #[test]
    fn tracked_surface_manifest_covers_exact_scenario_matrix() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let (_, scenarios, policies, _) =
            load_manifest(repo, &repo.join("opqual/scenario-surfaces-v1.json")).unwrap();
        let ids = matrix_ids(&repo.join("opqual/scenario-map.csv")).unwrap();
        assert_eq!(ids, scenarios.keys().cloned().collect());
        assert_eq!(ids.len(), 195);
        assert!(scenarios.values().all(|name| policies.contains_key(name)));
    }
}
