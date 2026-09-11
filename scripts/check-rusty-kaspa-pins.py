#!/usr/bin/env python3
"""Fail closed unless reviewed rusty-kaspa dependencies use immutable exact pins."""

from __future__ import annotations

import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CARGO_TOML = ROOT / "Cargo.toml"
CARGO_LOCK = ROOT / "Cargo.lock"
DENY_TOML = ROOT / "deny.toml"
GIT_URL = "https://github.com/kaspanet/rusty-kaspa"
DIRECT_PACKAGES = (
    "kaspa-wrpc-client",
    "kaspa-rpc-core",
    "kaspa-addresses",
    "kaspa-consensus-core",
    "kaspa-hashes",
)
EXACT_VERSION = re.compile(r"^=([0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?)$")
FULL_SHA = re.compile(r"^[0-9a-f]{40}$")


def fail(message: str) -> "NoReturn":
    raise SystemExit(f"rusty-kaspa-pin-check: FAIL: {message}")


def main() -> int:
    cargo = tomllib.loads(CARGO_TOML.read_text(encoding="utf-8"))
    dependencies = cargo.get("dependencies", {})
    versions: set[str] = set()
    revisions: set[str] = set()

    for name in DIRECT_PACKAGES:
        spec = dependencies.get(name)
        if not isinstance(spec, dict):
            fail(f"{name} must be an inline/table dependency specification")
        if spec.get("git") != GIT_URL:
            fail(f"{name} must use reviewed Git source {GIT_URL}")
        if "tag" in spec or "branch" in spec:
            fail(f"{name} must not use mutable tag/branch references")
        version = spec.get("version")
        match = EXACT_VERSION.fullmatch(version or "")
        if not match:
            fail(f"{name} must use an exact '=x.y.z' version requirement")
        rev = spec.get("rev")
        if not isinstance(rev, str) or not FULL_SHA.fullmatch(rev):
            fail(f"{name} must use a full lowercase 40-character rev")
        versions.add(match.group(1))
        revisions.add(rev)

    if len(versions) != 1 or len(revisions) != 1:
        fail("all direct rusty-kaspa dependencies must share one exact version and revision")

    version = next(iter(versions))
    revision = next(iter(revisions))
    lock = tomllib.loads(CARGO_LOCK.read_text(encoding="utf-8"))
    locked = [
        package for package in lock.get("package", [])
        if GIT_URL in str(package.get("source", ""))
    ]
    if not locked:
        fail("Cargo.lock contains no rusty-kaspa packages")

    expected_source = f"git+{GIT_URL}?rev={revision}#{revision}"
    for package in locked:
        if package.get("source") != expected_source:
            fail(
                f"Cargo.lock source drift for {package.get('name', '<unknown>')}: "
                f"{package.get('source')}"
            )

    deny = tomllib.loads(DENY_TOML.read_text(encoding="utf-8"))
    if deny.get("bans", {}).get("wildcards") != "deny":
        fail("deny.toml must enforce bans.wildcards = 'deny'")
    allowed_git = deny.get("sources", {}).get("allow-git", [])
    if GIT_URL not in allowed_git:
        fail("deny.toml must explicitly allow the reviewed rusty-kaspa source")

    print(
        "rusty-kaspa-pin-check: PASS "
        f"version={version} rev={revision} locked_packages={len(locked)}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
