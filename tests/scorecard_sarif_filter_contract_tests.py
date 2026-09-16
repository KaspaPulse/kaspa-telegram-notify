#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SCRIPT = ROOT / "scripts" / "filter-scorecard-sarif.py"
SPEC = importlib.util.spec_from_file_location("scorecard_filter", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ScorecardSarifFilterContractTests(unittest.TestCase):
    def test_filters_only_solo_maintainer_governance_results(self) -> None:
        document = {
            "version": "2.1.0",
            "runs": [
                {
                    "results": [
                        {"ruleId": "CodeReviewID"},
                        {"ruleId": "BranchProtectionID"},
                        {"ruleId": "CIIBestPracticesID"},
                        {"ruleId": "VulnerabilitiesID"},
                        {"ruleId": "DangerousWorkflowID"},
                    ]
                }
            ],
        }
        removed = MODULE.filter_scorecard_sarif(document)
        kept = [item["ruleId"] for item in document["runs"][0]["results"]]

        self.assertEqual(
            kept,
            ["VulnerabilitiesID", "DangerousWorkflowID"],
        )
        self.assertEqual(removed["CodeReviewID"], 1)
        self.assertEqual(removed["BranchProtectionID"], 1)
        self.assertEqual(removed["CIIBestPracticesID"], 1)
        self.assertNotIn("VulnerabilitiesID", removed)

    def test_unknown_findings_are_preserved_fail_closed(self) -> None:
        document = {
            "runs": [
                {
                    "results": [
                        {"ruleId": "FutureSecurityFindingID"},
                        {"ruleId": "CodeReviewID"},
                    ]
                }
            ]
        }
        MODULE.filter_scorecard_sarif(document)
        self.assertEqual(
            document["runs"][0]["results"],
            [{"ruleId": "FutureSecurityFindingID"}],
        )
    def test_malformed_sarif_fails_closed(self) -> None:
        with self.assertRaisesRegex(ValueError, "runs array"):
            MODULE.filter_scorecard_sarif({})

    def test_round_trip_keeps_valid_json(self) -> None:
        document = {"runs": [{"results": [{"ruleId": "CodeReviewID"}]}]}
        with tempfile.TemporaryDirectory() as temp_dir:
            source = Path(temp_dir) / "source.sarif"
            target = Path(temp_dir) / "target.sarif"
            source.write_text(json.dumps(document), encoding="utf-8")
            loaded = json.loads(source.read_text(encoding="utf-8"))
            MODULE.filter_scorecard_sarif(loaded)
            target.write_text(json.dumps(loaded), encoding="utf-8")
            self.assertEqual(
                json.loads(target.read_text(encoding="utf-8"))["runs"][0]["results"],
                [],
            )


if __name__ == "__main__":
    unittest.main()
