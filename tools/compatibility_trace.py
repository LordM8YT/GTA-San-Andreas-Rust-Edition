"""Compare measured GTA:SA reference checkpoints with SARE candidate traces.

Usage:
    python tools/compatibility_trace.py reference.json candidate.json

The reference file defines expected values and tolerances. Candidate values are
measurements captured from SARE using the same scenario and checkpoint IDs.
No game code or proprietary assets are needed by this tool.
"""
from __future__ import annotations

import json
import math
import sys
from pathlib import Path
from typing import Any


def _load(path: str | Path) -> dict[str, Any]:
    try:
        data = json.loads(Path(path).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ValueError(f"cannot read {path}: {exc}") from exc
    if not isinstance(data, dict):
        raise ValueError(f"{path}: top-level JSON value must be an object")
    return data


def _check_trace(trace: dict[str, Any], label: str) -> tuple[str, dict[str, dict[str, Any]]]:
    if trace.get("schema_version") != 1:
        raise ValueError(f"{label}: schema_version must be 1")
    scenario_id = trace.get("scenario_id")
    if not isinstance(scenario_id, str) or not scenario_id.strip():
        raise ValueError(f"{label}: scenario_id must be a non-empty string")
    raw_checkpoints = trace.get("checkpoints")
    if not isinstance(raw_checkpoints, list) or not raw_checkpoints:
        raise ValueError(f"{label}: checkpoints must be a non-empty array")

    checkpoints: dict[str, dict[str, Any]] = {}
    for index, checkpoint in enumerate(raw_checkpoints):
        if not isinstance(checkpoint, dict):
            raise ValueError(f"{label}: checkpoint {index} must be an object")
        checkpoint_id = checkpoint.get("id")
        values = checkpoint.get("values")
        if not isinstance(checkpoint_id, str) or not checkpoint_id.strip():
            raise ValueError(f"{label}: checkpoint {index} needs a non-empty id")
        if checkpoint_id in checkpoints:
            raise ValueError(f"{label}: duplicate checkpoint id {checkpoint_id!r}")
        if not isinstance(values, dict) or not values:
            raise ValueError(f"{label}: checkpoint {checkpoint_id!r} needs values")
        checkpoints[checkpoint_id] = values
    return scenario_id, checkpoints


def compare_traces(reference: dict[str, Any], candidate: dict[str, Any]) -> list[str]:
    """Return comparison failures; an empty list means all reference checks passed."""
    ref_id, ref_points = _check_trace(reference, "reference")
    candidate_id, candidate_points = _check_trace(candidate, "candidate")
    if ref_id != candidate_id:
        return [f"scenario_id mismatch: reference={ref_id!r}, candidate={candidate_id!r}"]

    tolerances = reference.get("tolerances", {})
    if not isinstance(tolerances, dict):
        return ["reference: tolerances must be an object"]

    failures: list[str] = []
    for checkpoint_id, expected_values in ref_points.items():
        actual_values = candidate_points.get(checkpoint_id)
        if actual_values is None:
            failures.append(f"{checkpoint_id}: missing candidate checkpoint")
            continue
        for metric, expected in expected_values.items():
            if metric not in actual_values:
                failures.append(f"{checkpoint_id}.{metric}: missing candidate measurement")
                continue
            actual = actual_values[metric]
            label = f"{checkpoint_id}.{metric}"
            if isinstance(expected, bool):
                if not isinstance(actual, bool) or actual is not expected:
                    failures.append(f"{label}: expected {expected!r}, got {actual!r}")
                continue
            if isinstance(expected, (int, float)) and not isinstance(expected, bool):
                if not isinstance(actual, (int, float)) or isinstance(actual, bool):
                    failures.append(f"{label}: expected numeric value, got {actual!r}")
                    continue
                if not math.isfinite(float(expected)) or not math.isfinite(float(actual)):
                    failures.append(f"{label}: values must be finite numbers")
                    continue
                tolerance = tolerances.get(metric)
                if not isinstance(tolerance, (int, float)) or isinstance(tolerance, bool):
                    failures.append(f"{label}: reference has no numeric tolerance for {metric!r}")
                    continue
                if not math.isfinite(float(tolerance)) or tolerance < 0:
                    failures.append(f"{label}: tolerance must be a finite non-negative number")
                    continue
                delta = abs(float(actual) - float(expected))
                if delta > float(tolerance):
                    failures.append(
                        f"{label}: expected {expected} ± {tolerance}, got {actual} "
                        f"(absolute error {delta:.6g})"
                    )
                continue
            if actual != expected or type(actual) is not type(expected):
                failures.append(f"{label}: expected {expected!r}, got {actual!r}")
    return failures


def main(argv: list[str] | None = None) -> int:
    args = list(sys.argv[1:] if argv is None else argv)
    if len(args) != 2:
        print("Usage: python tools/compatibility_trace.py reference.json candidate.json", file=sys.stderr)
        return 2
    try:
        reference = _load(args[0])
        candidate = _load(args[1])
        failures = compare_traces(reference, candidate)
    except ValueError as exc:
        print(f"INVALID TRACE: {exc}", file=sys.stderr)
        return 2

    if failures:
        print(f"COMPATIBILITY CHECK FAILED ({len(failures)} issue(s))")
        for failure in failures:
            print(f"- {failure}")
        return 1
    _, checkpoints = _check_trace(reference, "reference")
    count = sum(len(values) for values in checkpoints.values())
    print(f"COMPATIBILITY CHECK PASSED: {len(checkpoints)} checkpoints, {count} measurements")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
