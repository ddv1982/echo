#!/usr/bin/env python3
"""Reject incomplete or stale native verification evidence."""

import copy
import importlib.util
from pathlib import Path
import unittest


SPEC = importlib.util.spec_from_file_location(
    "recording_native", Path(__file__).with_name("verify-recording-native.py")
)
NATIVE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(NATIVE)


class NativeEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.report = {
            "commit": "tested-commit",
            "report": {
                "appVersion": "0.14.17",
                "verification": {
                    "checks": [{"name": f"contract-{i}", "passed": True} for i in range(10)],
                    "timingsMs": {
                        "startReceipt": 10,
                        "staleStopReceipt": 1,
                        "stopReceipt": 14,
                        "terminalObservation": 82,
                    },
                    "settingsRevisions": [1, 2, 3, 4],
                },
            },
        }

    def valid(self, report):
        return NATIVE.valid_contract_report(report, "tested-commit", "0.14.17")

    def test_complete_evidence_is_accepted(self):
        self.assertTrue(self.valid(self.report))

    def test_perf_only_output_does_not_pass_recording_verification(self):
        self.report["report"].pop("verification")
        self.assertFalse(self.valid(self.report))

    def test_stale_binary_identity_is_rejected(self):
        self.report["commit"] = "older-commit"
        self.assertFalse(self.valid(self.report))

    def test_failed_or_missing_contract_is_rejected(self):
        self.report["report"]["verification"]["checks"][0]["passed"] = False
        self.assertFalse(self.valid(self.report))
        self.report["report"]["verification"]["checks"].pop(0)
        self.assertFalse(self.valid(self.report))

    def test_invalid_measurements_and_revisions_are_rejected(self):
        invalid = copy.deepcopy(self.report)
        invalid["report"]["verification"]["timingsMs"]["stopReceipt"] = float("nan")
        self.assertFalse(self.valid(invalid))
        self.report["report"]["verification"]["settingsRevisions"] = [1, 3, 2, 4]
        self.assertFalse(self.valid(self.report))


if __name__ == "__main__":
    unittest.main()
