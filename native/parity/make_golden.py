"""Turn reference per-tick dumps into the compact golden file the tests read.

The reference dumps come from `ref_run.py`, which drives the validated Python
GTA kernel (infernus-physics) over the scenarios in `scenarios/`. Keeping the
generated file small means the parity claim is a normal `cargo test` assertion
rather than a script someone has to remember to run.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
KEEP = [1, 2, 3, 5, 10, 25, 50, 100, 150, 200, 300, 400, 600, 900]


def main(source: Path) -> int:
    names = sorted(p.stem.replace(".py", "") for p in source.glob("*.py.jsonl"))
    lines = [
        "# Reference state from the validated Python GTA kernel, per scenario.",
        "# scenario tick x y z vx vy vz rxx rxy rxz rfx rfy rfz rux ruy ruz",
        "# Regenerate with native/parity/make_golden.py against the reference package.",
    ]
    rows = 0
    for name in names:
        for line in (source / f"{name}.py.jsonl").read_text().splitlines():
            if not line.strip():
                continue
            record = json.loads(line)
            if record["tick"] not in KEEP:
                continue
            values = (
                record["position"]
                + record["move_speed"]
                + [x for row in record["basis"] for x in row]
            )
            lines.append(
                f"{name} {record['tick']} " + " ".join(f"{value:.9g}" for value in values)
            )
            rows += 1
    (HERE / "golden.txt").write_text("\n".join(lines) + "\n")
    print(f"wrote {rows} rows to {HERE / 'golden.txt'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(Path(sys.argv[1]) if len(sys.argv) > 1 else HERE / "out"))
