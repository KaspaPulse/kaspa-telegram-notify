use anyhow::{Context, Result, bail, ensure};
use regex::Regex;
use std::cmp::Ordering;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const KASPA_REMOTE: &str = "https://github.com/kaspanet/rusty-kaspa.git";
const KASPA_SOURCE: &str = "https://github.com/kaspanet/rusty-kaspa";
const PACKAGES: [&str; 5] = [
    "kaspa-wrpc-client",
    "kaspa-rpc-core",
    "kaspa-addresses",
    "kaspa-consensus-core",
    "kaspa-hashes",
];

#[derive(Clone, Debug, Eq, PartialEq)]
struct Version {
    tag: String,
    major: u64,
    minor: u64,
    patch: u64,
    prerelease: Option<String>,
}

impl Version {
    fn parse(tag: &str) -> Result<Self> {
        let re = Regex::new(r"^v(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$")?;
        let c = re
            .captures(tag)
            .with_context(|| format!("invalid Kaspa tag: {tag}"))?;
        Ok(Self {
            tag: tag.to_owned(),
            major: c[1].parse()?,
            minor: c[2].parse()?,
            patch: c[3].parse()?,
            prerelease: c.get(4).map(|m| m.as_str().to_owned()),
        })
    }

    fn version(&self) -> &str {
        self.tag.strip_prefix('v').unwrap_or(&self.tag)
    }

    fn stable(&self) -> bool {
        self.prerelease.is_none()
    }

    fn cmp_release(&self, other: &Self) -> Ordering {
        self.major
            .cmp(&other.major)
            .then(self.minor.cmp(&other.minor))
            .then(self.patch.cmp(&other.patch))
            .then_with(|| compare_prerelease(&self.prerelease, &other.prerelease))
    }
}

fn compare_prerelease(left: &Option<String>, right: &Option<String>) -> Ordering {
    match (left, right) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(left), Some(right)) => compare_prerelease_text(left, right),
    }
}
fn compare_prerelease_text(left: &str, right: &str) -> Ordering {
    for (a, b) in left.split('.').zip(right.split('.')) {
        let ordering = match (a.parse::<u64>(), b.parse::<u64>()) {
            (Ok(a), Ok(b)) => a.cmp(&b),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => a.cmp(b),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.split('.').count().cmp(&right.split('.').count())
}

fn output(program: &str, args: &[&str]) -> Result<String> {
    let result = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .with_context(|| format!("failed to execute {program}"))?;
    ensure!(
        result.status.success(),
        "{program} failed with {}",
        result.status
    );
    String::from_utf8(result.stdout).context("command output was not UTF-8")
}

fn success(program: &str, args: &[&str]) -> Result<bool> {
    Ok(Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("failed to execute {program}"))?
        .success())
}

fn run(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("failed to execute {program}"))?;
    ensure!(status.success(), "{program} failed with {status}");
    Ok(())
}

fn current_branch() -> Result<String> {
    let branch = output("git", &["branch", "--show-current"])?;
    let branch = branch.trim().to_owned();
    ensure!(!branch.is_empty(), "no current Git branch");
    Ok(branch)
}

fn ensure_clean() -> Result<()> {
    ensure!(
        output("git", &["status", "--porcelain"])?.trim().is_empty(),
        "working tree is not clean; refusing automatic dependency mutation"
    );
    Ok(())
}

fn latest_from_remote(text: &str, allow_prerelease: bool) -> Result<Version> {
    let mut versions = Vec::new();
    for line in text.lines() {
        let Some(reference) = line.split_whitespace().nth(1) else {
            continue;
        };
        let Some(tag) = reference.strip_prefix("refs/tags/") else {
            continue;
        };
        let Ok(version) = Version::parse(tag) else {
            continue;
        };
        if allow_prerelease || version.stable() {
            versions.push(version);
        }
    }
    versions
        .into_iter()
        .max_by(|a, b| a.cmp_release(b))
        .context("no eligible rusty-kaspa semver tags found")
}

fn resolve_revision(text: &str, tag: &str) -> Result<String> {
    let direct_ref = format!("refs/tags/{tag}");
    let peeled_ref = format!("{direct_ref}^{{}}");
    let sha = Regex::new(r"^[0-9a-fA-F]{40}$")?;
    let mut direct = None;
    let mut peeled = None;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let mut fields = line.split_whitespace();
        let revision = fields.next().context("missing tag revision")?;
        let reference = fields.next().context("missing tag reference")?;
        ensure!(
            fields.next().is_none() && sha.is_match(revision),
            "invalid tag result"
        );
        if reference == direct_ref {
            direct = Some(revision.to_ascii_lowercase());
        } else if reference == peeled_ref {
            peeled = Some(revision.to_ascii_lowercase());
        } else {
            bail!("unexpected tag reference: {reference}");
        }
    }
    let direct = direct.with_context(|| format!("no direct tag ref for {tag}"))?;
    Ok(peeled.unwrap_or(direct))
}

fn latest_remote(allow_prerelease: bool) -> Result<(Version, String)> {
    let tags = output(
        "git",
        &[
            "ls-remote",
            "--tags",
            "--refs",
            KASPA_REMOTE,
            "refs/tags/v*",
        ],
    )?;
    let latest = latest_from_remote(&tags, allow_prerelease)?;
    let direct = format!("refs/tags/{}", latest.tag);
    let peeled = format!("{direct}^{{}}");
    let resolved = output(
        "git",
        &["ls-remote", "--tags", KASPA_REMOTE, &direct, &peeled],
    )?;
    let revision = resolve_revision(&resolved, &latest.tag)?;
    Ok((latest, revision))
}

fn current_pins() -> Result<(Version, String)> {
    let cargo = fs::read_to_string("Cargo.toml").context("failed to read Cargo.toml")?;
    let source = format!("git = \"{KASPA_SOURCE}\"");
    ensure!(
        cargo.matches(&source).count() == PACKAGES.len(),
        "expected exactly {} direct rusty-kaspa dependencies",
        PACKAGES.len()
    );
    let mut version = None::<String>;
    let mut revision = None::<String>;
    for package in PACKAGES {
        let re = Regex::new(&format!(
            r#"(?m)^{}\s*=\s*\{{\s*version\s*=\s*"=([^"]+)"\s*,\s*git\s*=\s*"https://github\.com/kaspanet/rusty-kaspa"\s*,\s*rev\s*=\s*"([0-9a-fA-F]{{40}})"\s*\}}\s*$"#,
            regex::escape(package)
        ))?;
        let captures = re
            .captures(&cargo)
            .with_context(|| format!("{package} is not exactly pinned"))?;
        let found_version = captures[1].to_owned();
        let found_revision = captures[2].to_ascii_lowercase();
        if let Some(expected) = &version {
            ensure!(
                expected == &found_version,
                "inconsistent rusty-kaspa versions"
            );
        } else {
            version = Some(found_version);
        }
        if let Some(expected) = &revision {
            ensure!(
                expected == &found_revision,
                "inconsistent rusty-kaspa revisions"
            );
        } else {
            revision = Some(found_revision);
        }
    }
    let version = version.context("missing rusty-kaspa version")?;
    let revision = revision.context("missing rusty-kaspa revision")?;
    Ok((Version::parse(&format!("v{version}"))?, revision))
}

fn replace_pins(input: &str, target: &Version, revision: &str) -> Result<String> {
    ensure!(
        Regex::new(r"^[0-9a-f]{40}$")?.is_match(revision),
        "target revision must be a full lowercase commit SHA"
    );
    let mut value = input.to_owned();
    for package in PACKAGES {
        let re = Regex::new(&format!(
            r#"(?m)^({}\s*=\s*\{{\s*version\s*=\s*")=[^"]+("\s*,\s*git\s*=\s*"https://github\.com/kaspanet/rusty-kaspa"\s*,\s*rev\s*=\s*")[0-9a-fA-F]{{40}}("\s*\}}\s*)$"#,
            regex::escape(package)
        ))?;
        let before = value.clone();
        value = re
            .replace(&value, |captures: &regex::Captures<'_>| {
                format!(
                    "{}={}{}{}{}",
                    &captures[1],
                    target.version(),
                    &captures[2],
                    revision,
                    &captures[3]
                )
            })
            .into_owned();
        ensure!(
            value != before,
            "no exact version/revision pin changed for {package}"
        );
    }
    Ok(value)
}

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .context("bad path")?;
    let temporary = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .with_context(|| format!("failed to create {}", temporary.display()))?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, path)
        .with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

fn run_cargo(args: &[&str]) -> Result<()> {
    let status = Command::new("cargo")
        .args(args)
        .env("SQLX_OFFLINE", "true")
        .env("CARGO_INCREMENTAL", "0")
        .env("RUST_BACKTRACE", "1")
        .status()
        .context("failed to execute cargo")?;
    ensure!(status.success(), "cargo failed with {status}");
    Ok(())
}
fn validate_candidate(version: &str, revision: &str) -> Result<()> {
    run_cargo(&["fmt", "--all", "--", "--check"])?;
    run_cargo(&["check", "--locked", "--all-targets", "--all-features"])?;
    run_cargo(&[
        "clippy",
        "--locked",
        "--all-targets",
        "--all-features",
        "--",
        "-D",
        "warnings",
    ])?;
    run_cargo(&["test", "--locked", "--all-targets", "--all-features"])?;
    ensure!(
        success("cargo-audit", &["--version"])?,
        "cargo-audit is required"
    );
    ensure!(
        success("cargo-deny", &["--version"])?,
        "cargo-deny is required"
    );
    run("cargo", &["audit"])?;
    run("cargo", &["deny", "check"])?;
    crate::security::kaspa_pins(".", Some(version), Some(revision))?;
    let exe = std::env::current_exe().context("cannot identify xtask executable")?;
    let status = Command::new(exe)
        .args(["security", "pipeline"])
        .status()
        .context("failed to run Rust security pipeline")?;
    ensure!(status.success(), "Rust security pipeline failed");
    run("git", &["diff", "--check"])?;
    Ok(())
}

pub fn check(allow_prerelease: bool) -> Result<()> {
    let (current, current_revision) = current_pins()?;
    let (latest, latest_revision) = latest_remote(allow_prerelease)?;
    ensure!(
        current.cmp_release(&latest) != Ordering::Greater,
        "refusing downgrade: current {} is newer than upstream {}",
        current.tag,
        latest.tag
    );
    if current.tag == latest.tag {
        ensure!(
            current_revision == latest_revision,
            "SECURITY: upstream tag drift for {}: pinned {} != upstream {}",
            current.tag,
            current_revision,
            latest_revision
        );
    }
    println!(
        "rusty-kaspa-upstream-check: PASS current={}@{} latest={}@{}",
        current.tag, current_revision, latest.tag, latest_revision
    );
    Ok(())
}

pub fn update(allow_prerelease: bool, no_branch: bool, base_branch: &str) -> Result<()> {
    ensure_clean()?;
    let (current, current_revision) = current_pins()?;
    let (latest, latest_revision) = latest_remote(allow_prerelease)?;
    ensure!(
        current.cmp_release(&latest) != Ordering::Greater,
        "refusing downgrade from {} to {}",
        current.tag,
        latest.tag
    );
    if current.tag == latest.tag {
        ensure!(
            current_revision == latest_revision,
            "SECURITY: upstream tag drift for {}: pinned {} != upstream {}",
            current.tag,
            current_revision,
            latest_revision
        );
        println!(
            "rusty-kaspa-update: UP_TO_DATE {}@{}",
            latest.tag, latest_revision
        );
        return Ok(());
    }

    let update_branch = if no_branch {
        current_branch()?
    } else {
        let branch = current_branch()?;
        ensure!(
            branch == base_branch,
            "expected checked-out base branch {base_branch}, found {branch}"
        );
        let safe_tag = latest
            .tag
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-') {
                    ch
                } else {
                    '-'
                }
            })
            .collect::<String>();
        let update_branch = format!("auto/rusty-kaspa-{safe_tag}");
        ensure!(
            output("git", &["branch", "--list", &update_branch])?
                .trim()
                .is_empty(),
            "local update branch already exists: {update_branch}"
        );
        run("git", &["checkout", "-b", &update_branch])?;
        update_branch
    };
    let cargo_path = Path::new("Cargo.toml");
    let cargo = fs::read_to_string(cargo_path)?;
    let updated = replace_pins(&cargo, &latest, &latest_revision)?;
    atomic_write(cargo_path, &updated)?;

    run_cargo(&["check", "--all-targets", "--all-features"])?;
    validate_candidate(latest.version(), &latest_revision)?;
    println!(
        "rusty-kaspa-update: PASS branch={} target={}@{}",
        update_branch, latest.tag, latest_revision
    );
    Ok(())
}

pub fn publish(base_branch: &str) -> Result<()> {
    let branch = current_branch()?;
    if branch == base_branch {
        println!("rusty-kaspa-publish: NO_UPDATE branch={base_branch}");
        return Ok(());
    }
    ensure!(
        branch.starts_with("auto/rusty-kaspa-v"),
        "refusing to publish unexpected branch: {branch}"
    );
    ensure!(success("gh", &["--version"])?, "GitHub CLI is required");

    let existing_pr = output(
        "gh",
        &[
            "pr",
            "list",
            "--head",
            &branch,
            "--base",
            base_branch,
            "--state",
            "open",
            "--json",
            "number",
            "--jq",
            ".[0].number",
        ],
    )?;
    if !existing_pr.trim().is_empty() {
        println!(
            "rusty-kaspa-publish: EXISTING_PR number={}",
            existing_pr.trim()
        );
        return Ok(());
    }

    let remote_ref = format!("refs/heads/{branch}");
    if success(
        "git",
        &["ls-remote", "--exit-code", "--heads", "origin", &remote_ref],
    )? {
        bail!("remote update branch {branch} exists without an open PR; refusing overwrite");
    }

    run("git", &["config", "user.name", "kaspa-pulse-auto-updater"])?;
    run(
        "git",
        &[
            "config",
            "user.email",
            "41898282+github-actions[bot]@users.noreply.github.com",
        ],
    )?;
    run("git", &["add", "Cargo.toml", "Cargo.lock"])?;
    let staged = output("git", &["diff", "--cached", "--name-only"])?;
    let paths = staged
        .lines()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    ensure!(!paths.is_empty(), "no Cargo update changes are staged");
    ensure!(
        paths
            .iter()
            .all(|path| matches!(*path, "Cargo.toml" | "Cargo.lock")),
        "unexpected staged paths: {}",
        paths.join(", ")
    );
    ensure!(
        paths.contains(&"Cargo.toml"),
        "Cargo.toml is not staged for updater publication"
    );
    run("git", &["commit", "-m", "chore(deps): update rusty-kaspa"])?;
    run("gh", &["auth", "setup-git"])?;
    run("git", &["push", "--set-upstream", "origin", &branch])?;
    run(
        "gh",
        &[
            "pr",
            "create",
            "--base",
            base_branch,
            "--head",
            &branch,
            "--title",
            "Auto update rusty-kaspa dependencies",
            "--body",
            "Automated rusty-kaspa update. Rust-native qualification passed before publication.",
        ],
    )?;
    println!("rusty-kaspa-publish: PASS branch={branch}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_stable_and_prerelease() {
        let stable = Version::parse("v2.1.0").unwrap();
        let rc = Version::parse("v2.2.0-rc.1").unwrap();
        assert!(stable.stable());
        assert!(!rc.stable());
        assert_eq!(stable.version(), "2.1.0");
    }

    #[test]
    fn semver_prerelease_numeric_order_is_correct() {
        let rc2 = Version::parse("v2.2.0-rc.2").unwrap();
        let rc10 = Version::parse("v2.2.0-rc.10").unwrap();
        let stable = Version::parse("v2.2.0").unwrap();
        assert_eq!(rc10.cmp_release(&rc2), Ordering::Greater);
        assert_eq!(stable.cmp_release(&rc10), Ordering::Greater);
    }

    #[test]
    fn latest_stable_excludes_prerelease() {
        let input = concat!(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\trefs/tags/v2.0.0\n",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\trefs/tags/v2.1.0\n",
            "cccccccccccccccccccccccccccccccccccccccc\trefs/tags/v2.2.0-rc.1\n",
        );
        assert_eq!(latest_from_remote(input, false).unwrap().tag, "v2.1.0");
        assert_eq!(latest_from_remote(input, true).unwrap().tag, "v2.2.0-rc.1");
    }

    #[test]
    fn annotated_tag_uses_peeled_commit() {
        let input = concat!(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\trefs/tags/v2.1.0\n",
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\trefs/tags/v2.1.0^{}\n",
        );
        assert_eq!(
            resolve_revision(input, "v2.1.0").unwrap(),
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        );
    }

    #[test]
    fn pin_replacement_updates_all_five_packages() {
        let input = PACKAGES
            .iter()
            .map(|name| {
                format!(
                    "{name} = {{ version = \"=2.0.0\", git = \"{KASPA_SOURCE}\", rev = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" }}"
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let target = Version::parse("v2.1.0").unwrap();
        let output =
            replace_pins(&input, &target, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap();
        assert_eq!(output.matches("version = \"=2.1.0\"").count(), 5);
        assert_eq!(
            output
                .matches("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb")
                .count(),
            5
        );
    }
}
