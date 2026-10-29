"""Unit tests for the clean-room compatibility trace comparator."""
from __future__ import annotations

import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))

from compatibility_trace import compare_traces  # noqa: E402


def trace(scenario_id: str, checkpoints: list[dict], tolerances: dict | None = None) -> dict:
    result = {
        "schema_version": 1,
        "scenario_id": scenario_id,
        "checkpoints": checkpoints,
    }
    if tolerances is not None:
        result["tolerances"] = tolerances
    return result


class CompatibilityTraceTests(unittest.TestCase):
    def test_numeric_measurements_within_tolerance_pass(self) -> None:
        reference = trace(
            "vehicle/straight-line",
            [{"id": "after_2s", "values": {"speed_mps": 20.0, "grounded": True}}],
            {"speed_mps": 1.5},
        )
        candidate = trace(
            "vehicle/straight-line",
            [{"id": "after_2s", "values": {"speed_mps": 21.0, "grounded": True}}],
        )
        self.assertEqual(compare_traces(reference, candidate), [])

    def test_numeric_regression_fails_with_metric_name(self) -> None:
        reference = trace(
            "vehicle/straight-line",
            [{"id": "after_2s", "values": {"speed_mps": 20.0}}],
            {"speed_mps": 0.5},
        )
        candidate = trace(
            "vehicle/straight-line",
            [{"id": "after_2s", "values": {"speed_mps": 22.0}}],
        )
        failures = compare_traces(reference, candidate)
        self.assertEqual(len(failures), 1)
        self.assertIn("after_2s.speed_mps", failures[0])

    def test_missing_checkpoint_and_measurement_are_reported(self) -> None:
        reference = trace(
            "vehicle/landing",
            [
                {"id": "touchdown", "values": {"grounded": True}},
                {"id": "settled", "values": {"vertical_speed_mps": 0.0}},
            ],
            {"vertical_speed_mps": 0.1},
        )
        candidate = trace(
            "vehicle/landing",
            [{"id": "touchdown", "values": {}}],
        )
        failures = compare_traces(reference, candidate)
        self.assertTrue(any("touchdown.grounded" in item for item in failures))
        self.assertTrue(any("settled: missing candidate checkpoint" in item for item in failures))

    def test_scenario_mismatch_fails(self) -> None:
        reference = trace("vehicle/a", [{"id": "start", "values": {"grounded": True}}])
        candidate = trace("vehicle/b", [{"id": "start", "values": {"grounded": True}}])
        self.assertIn("scenario_id mismatch", compare_traces(reference, candidate)[0])

    def test_boolean_values_are_not_treated_as_numbers(self) -> None:
        reference = trace("vehicle/landing", [{"id": "touchdown", "values": {"grounded": True}}])
        candidate = trace("vehicle/landing", [{"id": "touchdown", "values": {"grounded": 1}}])
        self.assertIn("expected True, got 1", compare_traces(reference, candidate)[0])

    def test_numeric_measurement_requires_explicit_tolerance(self) -> None:
        reference = trace("vehicle/straight-line", [{"id": "after_2s", "values": {"speed_mps": 20.0}}])
        candidate = trace("vehicle/straight-line", [{"id": "after_2s", "values": {"speed_mps": 20.0}}])
        self.assertIn("no numeric tolerance", compare_traces(reference, candidate)[0])

    def test_duplicate_checkpoint_ids_are_rejected(self) -> None:
        duplicate = trace(
            "vehicle/a",
            [{"id": "start", "values": {"grounded": True}}, {"id": "start", "values": {"grounded": True}}],
        )
        with self.assertRaisesRegex(ValueError, "duplicate checkpoint"):
            compare_traces(duplicate, duplicate)


if __name__ == "__main__":
    unittest.main()
