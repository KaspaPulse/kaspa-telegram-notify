#!/usr/bin/env python3
"""Fail CI when RustSec/OSV exceptions are undocumented, stale, or over-broad."""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from datetime import date, datetime, timedelta, timezone
from pathlib import Path

RUSTSEC_PATTERN = re.compile(r"RUSTSEC-\d{4}-\d{4}")
REVIEW_PATTERN = re.compile(r"Last automated review:\s*\*\*(\d{4}-\d{2}-\d{2})\*\*")
MIN_REASON_LENGTH = 40


def fail(message: str) -> None:
    print(f"security-advisory-check: {message}", file=sys.stderr)
    raise SystemExit(1)


def parse_expiry(value: object, advisory: str) -> date:
    if isinstance(value, datetime):
        return value.date()
    if isinstance(value, date):
        return value
    if isinstance(value, str):
        try:
            return date.fromisoformat(value)
        except ValueError as exc:
            fail(f"invalid ignoreUntil for {advisory}: {exc}")
    fail(f"{advisory} must define ignoreUntil as YYYY-MM-DD")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--max-age-days", type=int, default=45)
    args = parser.parse_args()

    if args.max_age_days < 1:
        fail("--max-age-days must be positive")

    audit_path = Path(".cargo/audit.toml")
    osv_path = Path("osv-scanner.toml")
    report_path = Path("SECURITY_ADVISORIES.md")

    audit = audit_path.read_text(encoding="utf-8")
    report = report_path.read_text(encoding="utf-8")

    ignored_ids = sorted(set(RUSTSEC_PATTERN.findall(audit)))
    undocumented = [advisory for advisory in ignored_ids if advisory not in report]
    if undocumented:
        fail("undocumented cargo-audit ignored RustSec IDs: " + ", ".join(undocumented))

    match = REVIEW_PATTERN.search(report)
    if not match:
        fail("SECURITY_ADVISORIES.md is missing 'Last automated review: **YYYY-MM-DD**'")

    try:
        reviewed = date.fromisoformat(match.group(1))
    except ValueError as exc:
        fail(f"invalid review date: {exc}")

    today = datetime.now(timezone.utc).date()
    age_days = (today - reviewed).days
    if age_days < 0:
        fail(f"review date {reviewed.isoformat()} is in the future")
    if age_days > args.max_age_days:
        fail(
            f"security advisory review is {age_days} days old; "
            f"maximum allowed age is {args.max_age_days} days"
        )

    try:
        osv = tomllib.loads(osv_path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as exc:
        fail(f"cannot parse {osv_path}: {exc}")

    entries = osv.get("IgnoredVulns", [])
    if not isinstance(entries, list):
        fail("osv-scanner.toml IgnoredVulns must be an array of tables")

    osv_ids: list[str] = []
    latest_allowed_expiry = today + timedelta(days=args.max_age_days)
    for index, entry in enumerate(entries, start=1):
        if not isinstance(entry, dict):
            fail(f"IgnoredVulns entry #{index} must be a table")

        advisory = entry.get("id")
        if not isinstance(advisory, str) or RUSTSEC_PATTERN.fullmatch(advisory) is None:
            fail(f"IgnoredVulns entry #{index} must use a concrete RUSTSEC-YYYY-NNNN id")
        if advisory in osv_ids:
            fail(f"duplicate OSV ignored advisory: {advisory}")
        if advisory not in report:
            fail(f"OSV ignored advisory is not documented in SECURITY_ADVISORIES.md: {advisory}")

        reason = entry.get("reason")
        if not isinstance(reason, str) or len(reason.strip()) < MIN_REASON_LENGTH:
            fail(
                f"{advisory} reason must contain at least {MIN_REASON_LENGTH} "
                "non-whitespace characters"
            )

        expiry = parse_expiry(entry.get("ignoreUntil"), advisory)
        if expiry < today:
            fail(f"OSV exception expired for {advisory} on {expiry.isoformat()}")
        if expiry > latest_allowed_expiry:
            fail(
                f"OSV exception for {advisory} expires {expiry.isoformat()}, "
                f"more than {args.max_age_days} days from today"
            )

        osv_ids.append(advisory)

    print(
        "security-advisory-check: PASS "
        f"({len(ignored_ids)} cargo-audit ignores documented; "
        f"{len(osv_ids)} time-bounded OSV exceptions validated; "
        f"review age {age_days} days)"
    )


if __name__ == "__main__":
    main()
