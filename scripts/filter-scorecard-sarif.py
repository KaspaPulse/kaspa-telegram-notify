#!/usr/bin/env python3
"""Filter non-actionable solo-maintainer governance findings from Scorecard SARIF.

The canonical/full OpenSSF Scorecard result remains unfiltered and published.
This filter affects only the SARIF uploaded to GitHub Code Scanning.
"""

from __future__ import annotations

import argparse
import json
from collections import Counter
from pathlib import Path

EXCLUDED_RULE_IDS = frozenset(
    {
        "BranchProtectionID",
        "CodeReviewID",
        "CIIBestPracticesID",
    }
)


def filter_scorecard_sarif(document: dict) -> Counter[str]:
    """Remove only the explicitly approved governance rule results."""
    runs = document.get("runs")
    if not isinstance(runs, list):
        raise ValueError("SARIF document must contain a runs array")
    removed: Counter[str] = Counter()
    for run in runs:
        if not isinstance(run, dict):
            raise ValueError("every SARIF run must be an object")
        results = run.get("results", [])
        if not isinstance(results, list):
            raise ValueError("SARIF run results must be an array")

        kept = []
        for result in results:
            if not isinstance(result, dict):
                raise ValueError("every SARIF result must be an object")
            rule_id = result.get("ruleId")
            if rule_id in EXCLUDED_RULE_IDS:
                removed[str(rule_id)] += 1
                continue
            kept.append(result)
        run["results"] = kept

    return removed


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    document = json.loads(args.input.read_text(encoding="utf-8"))
    if not isinstance(document, dict):
        raise ValueError("SARIF root must be an object")

    removed = filter_scorecard_sarif(document)
    args.output.write_text(
        json.dumps(document, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )

    summary = ", ".join(
        f"{rule_id}={removed.get(rule_id, 0)}"
        for rule_id in sorted(EXCLUDED_RULE_IDS)
    )
    print(f"scorecard-sarif-filter: PASS ({summary})")


if __name__ == "__main__":
    main()
