"""CI checks for the clean-room compatibility harness registry."""
from __future__ import annotations

import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))

from compatibility_harnesses import load_manifest, validate_manifest  # noqa: E402


class CompatibilityHarnessRegistryTests(unittest.TestCase):
    def test_repository_manifest_is_valid(self) -> None:
        manifest = load_manifest()
        self.assertGreaterEqual(len(manifest["scenarios"]), 10)

    def test_scenario_ids_are_unique(self) -> None:
        manifest = load_manifest()
        ids = [scenario["id"] for scenario in manifest["scenarios"]]
        self.assertEqual(len(ids), len(set(ids)))

    def test_each_scenario_has_reproducible_inputs_and_checkpoints(self) -> None:
        manifest = load_manifest()
        for scenario in manifest["scenarios"]:
            with self.subTest(scenario=scenario["id"]):
                self.assertTrue(scenario["setup"])
                self.assertTrue(scenario["inputs"])
                self.assertTrue(scenario["checkpoints"])
                self.assertTrue(all(event["action"] for event in scenario["inputs"]))
                self.assertTrue(all(checkpoint["metrics"] for checkpoint in scenario["checkpoints"]))

    def test_duplicate_scenario_id_is_rejected(self) -> None:
        manifest = load_manifest()
        manifest["scenarios"].append(dict(manifest["scenarios"][0]))
        with self.assertRaisesRegex(ValueError, "duplicate scenario id"):
            validate_manifest(manifest)

    def test_duplicate_checkpoint_id_is_rejected(self) -> None:
        manifest = load_manifest()
        scenario = dict(manifest["scenarios"][0])
        scenario["checkpoints"] = scenario["checkpoints"] + [dict(scenario["checkpoints"][0])]
        manifest["scenarios"] = [scenario]
        with self.assertRaisesRegex(ValueError, "duplicate checkpoint"):
            validate_manifest(manifest)

    def test_missing_inputs_are_rejected(self) -> None:
        manifest = load_manifest()
        scenario = dict(manifest["scenarios"][0])
        scenario["inputs"] = []
        manifest["scenarios"] = [scenario]
        with self.assertRaisesRegex(ValueError, "inputs must be a non-empty list"):
            validate_manifest(manifest)

    def test_reference_cannot_be_claimed_captured_without_status(self) -> None:
        manifest = load_manifest()
        scenario = dict(manifest["scenarios"][0])
        scenario["reference_status"] = "captured"
        scenario["status"] = "spec_only"
        manifest["scenarios"] = [scenario]
        with self.assertRaisesRegex(ValueError, "captured reference needs"):
            validate_manifest(manifest)


if __name__ == "__main__":
    unittest.main()
