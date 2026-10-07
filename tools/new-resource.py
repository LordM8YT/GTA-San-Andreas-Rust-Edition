#!/usr/bin/env python3
"""Create a native SA resource with a familiar stream/data folder layout."""
import argparse
import json
from pathlib import Path
import re


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("name", help="Resource folder name, e.g. my_car")
    parser.add_argument("--type", choices=["vehicle", "map", "player", "clothing"], required=True)
    parser.add_argument("--mods-dir", type=Path, default=Path("mods"))
    args = parser.parse_args()
    reserved = {"con", "prn", "aux", "nul"} | {f"{prefix}{i}" for prefix in ("com", "lpt") for i in range(1, 10)}
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", args.name) or args.name.lower() in reserved:
        parser.error("Use 1-64 letters, numbers, underscores or hyphens; no reserved device names.")
    group = {"vehicle": "vehicles", "map": "maps", "player": "peds", "clothing": "clothing"}[args.type]
    folder = args.mods_dir / f"[{group}]" / args.name
    try:
        folder.resolve().relative_to(args.mods_dir.resolve())
    except ValueError:
        parser.error("Resource category escapes the mod directory through a link.")
    if folder.exists():
        parser.error(f"Resource already exists: {folder}; existing files will not be overwritten.")
    manifest = {"schema_version": 2, "enabled": False, "name": args.name}
    if args.type == "vehicle":
        manifest["vehicles"] = [{"name": args.name[:48], "dff": "stream/car.dff", "handling": {}}]
        assets = ["car.dff"]
    elif args.type == "map":
        manifest["models"] = [{"id": 30000, "dff": "stream/building.dff"}]
        manifest["placements"] = [{"model_id": 30000, "position": [2500, -1670, 12.35]}]
        assets = ["building.dff"]
    else:
        manifest["player"] = {"dff": "stream/ped.dff", "ifp": "stream/ped.ifp"}
        assets = ["ped.dff", "ped.ifp"]
        if args.type == "clothing":
            manifest["player"]["clothes"] = [{"name": "Jacket", "dff": "stream/jacket.dff", "enabled": True}]
            assets.append("jacket.dff")
    folder.mkdir(parents=True, exist_ok=False)
    (folder / "stream").mkdir()
    (folder / "data").mkdir()
    (folder / "resource.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    (folder / "README.md").write_text(
        f"# {args.name}\n\n"
        "Native SA resource. Disabled until you supply the assets.\n\n"
        + "Required files in `stream/`:\n\n"
        + "".join(f"- `{asset}`\n" for asset in assets)
        + "\nUse PC RenderWare DFF/TXD, COL collision and ANP3 IFP animation files. "
        "GTA V/FiveM binary assets and scripts do not load directly.\n\n"
        "Add `txd` paths for textured models. Maps can include a `col` path; choose "
        "an unused model ID and edit placements in `resource.json`. Custom players "
        "need matching bone IDs and idle_stance/walk_player/run_player clips; "
        "clothing needs the same bind pose as its player. Select one active vehicle "
        "and player from /cars and /peds.\n\n"
        "Set `enabled` to `true` in `resource.json` and restart the game. "
        "See `docs/native-mods.md` in the project for the full format.\n",
        encoding="utf-8",
    )
    (folder / "data" / "README.md").write_text(
        "# Data\n\nOptional authoring/source files belong here. Runtime placements "
        "and model registrations currently live in the root resource.json; external "
        "GTA V .meta files are not parsed.\n", encoding="utf-8",
    )
    print(f"Created disabled {args.type} resource: {folder}")


if __name__ == "__main__":
    main()
