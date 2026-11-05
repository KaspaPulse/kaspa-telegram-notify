#!/usr/bin/env python3
"""Normalize a generated CycloneDX JSON SBOM for deterministic attestation.

GitHub's actions/attest requires CycloneDX JSON to contain bomFormat,
specVersion, and serialNumber. cargo-cyclonedx 0.5.9 can emit a valid BOM
without serialNumber, so this script adds a deterministic UUIDv5 serial tied to
the repository, commit, package version, and build target.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import uuid
from pathlib import Path


def fail(message: str) -> None:
    print(f"cyclonedx-finalize: {message}", file=sys.stderr)
    raise SystemExit(1)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("path", type=Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", required=True)
    args = parser.parse_args()

    if not args.path.is_file() or args.path.stat().st_size == 0:
        fail(f"SBOM is missing or empty: {args.path}")

    repository = os.environ.get("GITHUB_REPOSITORY", "").strip()
    commit_sha = os.environ.get("GITHUB_SHA", "").strip().lower()
    if not repository:
        fail("GITHUB_REPOSITORY is required")
    if len(commit_sha) != 40 or any(ch not in "0123456789abcdef" for ch in commit_sha):
        fail("GITHUB_SHA must be a full 40-character Git commit SHA")

    try:
        document = json.loads(args.path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        fail(f"cannot parse CycloneDX JSON: {exc}")

    if not isinstance(document, dict):
        fail("CycloneDX document must be a JSON object")
    if document.get("bomFormat") != "CycloneDX":
        fail("bomFormat must be exactly 'CycloneDX'")

    spec_version = document.get("specVersion")
    if not isinstance(spec_version, str) or not spec_version.strip():
        fail("specVersion is required")

    seed = (
        f"https://github.com/{repository}@{commit_sha}"
        f"#kaspa-pulse:{args.version}:{args.target}:cyclonedx-{spec_version}"
    )
    serial_uuid = uuid.uuid5(uuid.NAMESPACE_URL, seed)
    document["serialNumber"] = f"urn:uuid:{serial_uuid}"

    # Deterministic serialization makes repeated builds of the same commit emit
    # the same normalized SBOM bytes, assuming the generator input is unchanged.
    args.path.write_text(
        json.dumps(document, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n",
        encoding="utf-8",
    )

    # Validate the exact fields actions/attest uses to recognize CycloneDX.
    normalized = json.loads(args.path.read_text(encoding="utf-8"))
    serial = normalized.get("serialNumber")
    if not isinstance(serial, str) or not serial.startswith("urn:uuid:"):
        fail("serialNumber normalization failed")
    try:
        uuid.UUID(serial.removeprefix("urn:uuid:"))
    except ValueError as exc:
        fail(f"invalid CycloneDX serialNumber UUID: {exc}")

    print(
        "cyclonedx-finalize: PASS "
        f"(specVersion={spec_version}, serialNumber={serial}, sha={commit_sha})"
    )


if __name__ == "__main__":
    main()
