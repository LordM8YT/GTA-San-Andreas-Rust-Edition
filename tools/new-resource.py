#!/usr/bin/env python3
"""Create a SARE resource or content pack to start from.

prop and world create working files at once (a starter .glb made by code);
vehicle, map, player and clothing create a disabled resource that waits for
your DFF/TXD files.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import re


def example_pack():
    """The generator behind content/example-town, reused for starter models."""
    spec = importlib.util.spec_from_file_location(
        "make_example_pack", Path(__file__).with_name("make-example-pack.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def starter_crate(path: Path):
    """A 1 m wooden-looking crate standing on its origin."""
    pack = example_pack()
    crate = pack.Mesh("Crate", material=1, color=(0.62, 0.44, 0.26))
    crate.box((-0.5, -0.5, 0.0), (0.5, 0.5, 1.0))
    pack.write_glb([crate], pack.ground_texture(), path)


def starter_world(path: Path):
    """A 200 m paved square with one block and a ramp."""
    pack = example_pack()
    ground = pack.Mesh("Ground", material=0)
    ground.quad([(-100, -100, 0), (100, -100, 0), (100, 100, 0), (-100, 100, 0)], (0, 0, 1), uv_scale=8.0)
    block = pack.Mesh("Block", material=1, color=(0.35, 0.55, 0.8))
    block.box((10, 10, 0), (20, 20, 6))
    ramp = pack.Mesh("Ramp", material=1, color=(0.95, 0.6, 0.2))
    ramp.ramp((-4, 4), (20, 40), (0, 3))
    edge = pack.Mesh("COL_Edge", material=1)
    for lo, hi in [((-101, -101, 0), (101, -100, 5)), ((-101, 100, 0), (101, 101, 5)),
                   ((-101, -100, 0), (-100, 100, 5)), ((100, -100, 0), (101, 100, 5))]:
        edge.box(lo, hi)
    pack.write_glb([ground, block, ramp, edge], pack.ground_texture(), path)


def create_world(parser, args):
    folder = args.content_dir / args.name
    if folder.exists():
        parser.error(f"Content pack already exists: {folder}; existing files will not be overwritten.")
    folder.mkdir(parents=True)
    starter_world(folder / "world.glb")
    manifest = {
        "id": args.name.lower(),
        "name": args.name.replace("_", " ").replace("-", " ").title(),
        "version": "0.1.0",
        "license": "CC0-1.0",
        "author": "",
        "description": "",
        "world": {"scene": "world.glb", "spawn": [0, 0, 0]},
    }
    (folder / "content.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    (folder / "README.md").write_text(
        f"# {manifest['name']}\n\n"
        "A SARE content pack: a world that runs without GTA San Andreas.\n\n"
        "1. Open `world.glb` in Blender (File > Import > glTF 2.0) and build your world.\n"
        "2. Export with File > Export > glTF 2.0, Format: glTF Binary, +Y Up, over `world.glb`.\n"
        "3. In the launcher press Refresh, then your pack's button.\n\n"
        "Objects named `COL_...` are invisible walls, `NOCOL_...` are not solid.\n"
        "Set your own `license` and `author` in `content.json` before sharing.\n"
        "Guide: docs/content-packs.md in the SARE repository.\n",
        encoding="utf-8",
    )
    print(f"Created content pack: {folder}  (play it: sa-runtime --content {manifest['id']})")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("name", help="Resource folder name, e.g. my_car")
    parser.add_argument("--type", choices=["prop", "world", "vehicle", "map", "player", "clothing"], required=True)
    parser.add_argument("--mods-dir", type=Path, default=Path("mods"))
    parser.add_argument("--content-dir", type=Path, default=Path("content"))
    args = parser.parse_args()
    reserved = {"con", "prn", "aux", "nul"} | {f"{prefix}{i}" for prefix in ("com", "lpt") for i in range(1, 10)}
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", args.name) or args.name.lower() in reserved:
        parser.error("Use 1-64 letters, numbers, underscores or hyphens; no reserved device names.")
    if args.type == "world":
        create_world(parser, args)
        return
    group = {"prop": "props", "vehicle": "vehicles", "map": "maps", "player": "peds", "clothing": "clothing"}[args.type]
    folder = args.mods_dir / f"[{group}]" / args.name
    try:
        folder.resolve().relative_to(args.mods_dir.resolve())
    except ValueError:
        parser.error("Resource category escapes the mod directory through a link.")
    if folder.exists():
        parser.error(f"Resource already exists: {folder}; existing files will not be overwritten.")
    manifest = {"schema_version": 2, "enabled": False, "name": args.name}
    if args.type == "prop":
        create_prop(folder, manifest, args.name)
        return
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


def create_prop(folder: Path, manifest: dict, name: str):
    manifest["enabled"] = True
    manifest["models"] = [{"id": 30000, "model": "stream/prop.glb"}]
    # Grove Street, next to where free roam starts.
    manifest["placements"] = [{"model_id": 30000, "position": [2495.0, -1665.0, 13.34]}]
    folder.mkdir(parents=True, exist_ok=False)
    (folder / "stream").mkdir()
    starter_crate(folder / "stream" / "prop.glb")
    (folder / "resource.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    (folder / "README.md").write_text(
        f"# {name}\n\n"
        "A SARE prop. It works right away: start free roam and look for a crate on Grove Street.\n\n"
        "1. Open `stream/prop.glb` in Blender (File > Import > glTF 2.0) and model your prop.\n"
        "   Keep the origin where the prop touches the ground. 1 unit = 1 metre.\n"
        "2. Export with File > Export > glTF 2.0, Format: glTF Binary, +Y Up, over `stream/prop.glb`.\n"
        "3. Move it by editing `placements` in `resource.json` (GTA X, Y, Z coordinates).\n"
        "   If it floats or sinks, change the last number (height). Use a model ID of 30000\n"
        "   or higher that no other mod uses.\n\n"
        "Textures go inside the .glb (PNG). Objects named `COL_...` are invisible but solid,\n"
        "`NOCOL_...` are drawn but not solid. Guide: docs/native-mods.md in the SARE repository.\n",
        encoding="utf-8",
    )
    print(f"Created prop resource: {folder}")


if __name__ == "__main__":
    main()
