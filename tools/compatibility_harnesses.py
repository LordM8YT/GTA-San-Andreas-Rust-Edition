"""List and validate clean-room GTA SA compatibility harness specifications."""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path
from typing import Any

TOOLS_DIR = Path(__file__).resolve().parent
MANIFEST_PATH = TOOLS_DIR / "compatibility_harnesses.json"
sys.path.insert(0, str(TOOLS_DIR))
from compatibility_trace import compare_traces  # noqa: E402


def load_manifest(path: Path = MANIFEST_PATH) -> dict[str, Any]:
    try:
        manifest = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        raise ValueError(f"cannot load harness manifest: {exc}") from exc
    validate_manifest(manifest)
    return manifest


def validate_manifest(manifest: Any) -> None:
    if not isinstance(manifest, dict) or manifest.get("schema_version") != 1:
        raise ValueError("manifest schema_version must be 1")
    scenarios = manifest.get("scenarios")
    if not isinstance(scenarios, list) or not scenarios:
        raise ValueError("manifest scenarios must be a non-empty list")
    seen: set[str] = set()
    for scenario in scenarios:
        if not isinstance(scenario, dict):
            raise ValueError("each scenario must be an object")
        for field in ("id", "subsystem", "title", "status", "reference_status", "setup", "inputs", "checkpoints"):
            if field not in scenario:
                raise ValueError(f"scenario is missing required field {field!r}")
        scenario_id = scenario["id"]
        if not isinstance(scenario_id, str) or not scenario_id.strip():
            raise ValueError("scenario id must be a non-empty string")
        if scenario_id in seen:
            raise ValueError(f"duplicate scenario id: {scenario_id}")
        seen.add(scenario_id)
        if scenario["status"] not in ("spec_only", "ready_for_capture", "reference_captured"):
            raise ValueError(f"{scenario_id}: invalid status {scenario['status']!r}")
        if scenario["reference_status"] not in ("not_captured", "captured"):
            raise ValueError(f"{scenario_id}: invalid reference_status")
        if not isinstance(scenario["setup"], dict) or not scenario["setup"]:
            raise ValueError(f"{scenario_id}: setup must be a non-empty object")
        inputs = scenario["inputs"]
        if not isinstance(inputs, list) or not inputs:
            raise ValueError(f"{scenario_id}: inputs must be a non-empty list")
        for event in inputs:
            if not isinstance(event, dict) or not {"at_s", "action", "value"} <= event.keys():
                raise ValueError(f"{scenario_id}: every input needs at_s, action and value")
        checkpoints = scenario["checkpoints"]
        if not isinstance(checkpoints, list) or not checkpoints:
            raise ValueError(f"{scenario_id}: checkpoints must be a non-empty list")
        checkpoint_ids: set[str] = set()
        for checkpoint in checkpoints:
            if not isinstance(checkpoint, dict) or not {"id", "at_s", "metrics"} <= checkpoint.keys():
                raise ValueError(f"{scenario_id}: every checkpoint needs id, at_s and metrics")
            checkpoint_id = checkpoint["id"]
            if not isinstance(checkpoint_id, str) or not checkpoint_id.strip():
                raise ValueError(f"{scenario_id}: checkpoint id must be non-empty")
            if checkpoint_id in checkpoint_ids:
                raise ValueError(f"{scenario_id}: duplicate checkpoint id {checkpoint_id!r}")
            checkpoint_ids.add(checkpoint_id)
            if not isinstance(checkpoint["metrics"], list) or not checkpoint["metrics"]:
                raise ValueError(f"{scenario_id}/{checkpoint_id}: metrics must be non-empty")
            if not all(isinstance(metric, str) and metric.strip() for metric in checkpoint["metrics"]):
                raise ValueError(f"{scenario_id}/{checkpoint_id}: metric names must be non-empty strings")
        if scenario["reference_status"] == "captured" and scenario["status"] != "reference_captured":
            raise ValueError(f"{scenario_id}: captured reference needs reference_captured status")


def compare_scenario(scenario_id: str, reference_path: Path, candidate_path: Path) -> list[str]:
    reference = json.loads(reference_path.read_text(encoding="utf-8"))
    candidate = json.loads(candidate_path.read_text(encoding="utf-8"))
    if reference.get("scenario_id") != scenario_id:
        return [f"reference scenario_id must be {scenario_id!r}"]
    if candidate.get("scenario_id") != scenario_id:
        return [f"candidate scenario_id must be {scenario_id!r}"]
    return compare_traces(reference, candidate)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("list", help="list the defined compatibility scenarios")
    validate = sub.add_parser("validate", help="validate the harness manifest")
    validate.add_argument("--manifest", type=Path, default=MANIFEST_PATH)
    compare = sub.add_parser("compare", help="compare captured reference and SARE traces")
    compare.add_argument("scenario_id")
    compare.add_argument("reference", type=Path)
    compare.add_argument("candidate", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.command in ("list", "validate"):
            manifest = load_manifest(args.manifest if args.command == "validate" else MANIFEST_PATH)
            if args.command == "list":
                for scenario in manifest["scenarios"]:
                    print(f"{scenario['id']} [{scenario['status']}; reference={scenario['reference_status']}] - {scenario['title']}")
                print(f"{len(manifest['scenarios'])} harness specifications")
            else:
                print(f"VALID: {len(manifest['scenarios'])} harness specifications")
            return 0
        failures = compare_scenario(args.scenario_id, args.reference, args.candidate)
    except (OSError, json.JSONDecodeError, ValueError) as exc:
        print(f"INVALID HARNESS INPUT: {exc}", file=sys.stderr)
        return 2
    if failures:
        print(f"COMPATIBILITY CHECK FAILED ({len(failures)} issue(s))")
        for failure in failures:
            print(f"- {failure}")
        return 1
    print(f"COMPATIBILITY CHECK PASSED: {args.scenario_id}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
