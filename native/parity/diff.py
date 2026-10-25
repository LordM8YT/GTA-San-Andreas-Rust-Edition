"""Diff the Rust kernel against the Python reference, tick by tick."""
from __future__ import annotations

import json
import os
import sys
from pathlib import Path

OUT = Path(__file__).resolve().parent / "out"
# Which Rust-side dump to compare against: the bare kernel (`rs`) or the scene
# layer driving the same scenario over flat ground (`scene`).
SIDE = os.environ.get("PARITY_SIDE", "rs")
FIELDS = ("position", "basis", "move_speed", "turn_speed", "compressions", "wheel_counts")


def flatten(row: dict, key: str) -> list[float]:
    value = row[key]
    if key == "basis":
        return [float(x) for r in value for x in r]
    if isinstance(value, list):
        return [float(x) for x in value]
    return [float(value)]


def main(names: list[str]) -> int:
    print(f"{'scenario':10} {'ticks':>6} {'first>1e-3':>26}  {'pos':>8} {'vel':>8} {'basis':>8} {'turn':>8} {'comp':>8} {'gear':>5}")
    worst_all = 0.0
    for name in names:
        py_path, rs_path = OUT / f"{name}.py.jsonl", OUT / f"{name}.{SIDE}.jsonl"
        if not py_path.exists() or not rs_path.exists():
            print(f"{name:10} missing output")
            continue
        py = [json.loads(l) for l in py_path.read_text().splitlines() if l.strip()]
        rs = [json.loads(l) for l in rs_path.read_text().splitlines() if l.strip()]
        n = min(len(py), len(rs))
        maxima = {k: 0.0 for k in FIELDS}
        first = None
        gear_mismatch = 0
        for i in range(n):
            for key in FIELDS:
                a, b = flatten(py[i], key), flatten(rs[i], key)
                delta = max(abs(x - y) for x, y in zip(a, b))
                maxima[key] = max(maxima[key], delta)
                if first is None and delta > 1.0e-3:
                    first = (i + 1, key, delta)
            if int(py[i]["gear"]) != int(rs[i]["gear"]):
                gear_mismatch += 1
        worst_all = max(worst_all, maxima["position"])
        label = "identical" if first is None else f"tick {first[0]} {first[1]} {first[2]:.4g}"
        print(
            f"{name:10} {n:6d} {label:>26}  "
            f"{maxima['position']:8.4f} {maxima['move_speed']:8.4f} {maxima['basis']:8.4f} "
            f"{maxima['turn_speed']:8.4f} {maxima['compressions']:8.4f} {gear_mismatch:5d}"
        )
    print(f"\nworst position error over all scenarios: {worst_all:.4f} m")
    return 0 if worst_all < 1.0 else 1


if __name__ == "__main__":
    names = sys.argv[1:] or ["settle", "accel", "brake", "corner", "slalom", "handbrake", "reverse", "air", "rollover"]
    raise SystemExit(main(names))
