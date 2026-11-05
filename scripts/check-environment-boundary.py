#!/usr/bin/env python3
"""Fail CI if the repository's kas/dns execution boundary is removed or weakened."""

from pathlib import Path

MARKER = "KAS_DNS_ENVIRONMENT_BOUNDARY_V1"

REQUIRED = {
    "AGENTS.md": [
        MARKER,
        "`kas` is the only development host.",
        "`dns` is the production host and is not a development environment.",
        "All source edits, dependency resolution, local Git operations, builds, tests, security scans, packaging, and release-artifact creation MUST run on `kas`.",
        "`dns` MUST NOT be configured or used as a CI runner.",
        "No source build is permitted on `dns` as part of deployment.",
        "Do not repair source code in place on production.",
    ],
    "CONTRIBUTING.md": [
        MARKER,
        "Development host: `kas` only.",
        "Production host: `dns` is deployment/verification only.",
    ],
}

for filename, needles in REQUIRED.items():
    text = Path(filename).read_text(encoding="utf-8")
    missing = [needle for needle in needles if needle not in text]
    if missing:
        raise SystemExit(f"environment-boundary-check: {filename} missing required policy: {missing!r}")

print("environment-boundary-check: PASS")
