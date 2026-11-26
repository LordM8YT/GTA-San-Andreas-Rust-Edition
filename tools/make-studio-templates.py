"""Write SARE Studio's starter templates, built only from SARE's own
built-in shapes and parts (CC0):

- content/obstacle-course: jump across floating platforms; fall in and you
  are back at the start; checkpoints time the run.
- content/racing-track: a closed road loop on a big field with checkpoints
  for cars, race results from a world script.

    python tools/make-studio-templates.py

SARE Client › Create copies a template into a new world of your own.
Coordinates are Blender axes (X east, Y north, Z up, metres); rotation is
degrees. See docs/build-mode.md for the layout format.
"""
import json
import math
import pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent / "content"

RACE_RESULTS = """-- Tells everyone each finish time of the course.
world.on('finish', function(player, race, seconds)
  world.message(player.name .. ' finished in ' .. string.format('%.1f', seconds) .. ' s', 6)
end)
"""


class Layout:
    def __init__(self):
        self.nodes = []

    def add(self, name, asset=None, at=(0, 0, 0), turn=0.0, parent=None, scale=None, **extra):
        node = {"id": len(self.nodes) + 1, "name": name}
        if parent:
            node["parent"] = parent
        if asset:
            node["asset"] = f"builtin:{asset}"
            node["transform"] = {
                "position": [round(v, 3) for v in at],
                "rotation": [0, 0, turn],
                "scale": list(scale or (1, 1, 1)),
            }
        node.update(extra)
        self.nodes.append(node)
        return node["id"]

    # A part's origin is the middle of its base (sa_scene::part).
    def part(self, name, at, size, color, material="plastic", parent=None, turn=0.0, shape="block"):
        return self.add(
            name, "part-" + shape.replace("_", "-"), at, turn, parent,
            part={"shape": shape, "size": list(size), "color": list(color), "material": material},
        )

    def write(self, folder, manifest, settings):
        folder.mkdir(parents=True, exist_ok=True)
        layout = {
            "format": "sare-world-layout",
            "version": 1,
            "next_id": len(self.nodes) + 1,
            "settings": settings,
            "nodes": self.nodes,
        }
        (folder / "layout.json").write_text(json.dumps(layout, indent=2) + "\n", encoding="utf-8")
        (folder / "content.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
        print(f"wrote {folder} ({len(self.nodes)} nodes)")


def message(text, seconds=4):
    return {"do": "message", "text": text, "seconds": seconds}


def obstacle_course():
    w = Layout()
    STONE = (0.62, 0.64, 0.68)
    COLORS = [(0.95, 0.35, 0.3), (0.98, 0.65, 0.2), (0.95, 0.85, 0.25), (0.4, 0.8, 0.4), (0.3, 0.6, 0.95), (0.65, 0.45, 0.9)]
    course = w.add("Course")
    w.part("Start platform", (0, 0, -1), (12, 12, 1), STONE, "concrete", course)
    # Platforms that climb and wind north; each a jump of 3-4 m.
    x, y, z = 0.0, 8.0, 0.0
    hops = []
    for i in range(14):
        y += 4.2
        x = 5.0 * math.sin(i * 0.9)
        z = min(i * 0.45, 4.5)
        size = 3.0 if i % 3 else 2.2
        hops.append((x, y, z))
        w.part(f"Platform {i + 1}", (x, y, z - 1), (size, size, 1), COLORS[i % len(COLORS)], "plastic", course)
    finish_y = y + 7.0
    w.part("Finish platform", (0, finish_y, z - 1), (10, 10, 1), (0.95, 0.85, 0.25), "metal", course)
    w.add("Flag pole", "pillar", (3, finish_y + 3, z), parent=course)
    # "Lava": a glowing floor under the course; the trigger below sends you back.
    w.part("Lava", (0, finish_y / 2, -7), (60, finish_y + 30, 1), (1.0, 0.3, 0.05), "smooth", course)

    play = w.add("Gameplay")
    start = w.add("Start", "marker-spawn", (0, -2, 0), parent=play, gameplay={"kind": "spawn"})
    back = w.add("Back to start", "marker-target", (0, -2, 0), parent=play)
    w.add(
        "Lava zone", "marker-trigger", (0, finish_y / 2, -6), parent=play,
        gameplay={
            "kind": "trigger",
            "size": [60, finish_y + 30, 3],
            "on_enter": [{"do": "teleport", "target": back}, message("Back to the start. Try again!", 3)],
        },
    )
    w.add(
        "Start line", "marker-checkpoint", (0, 4, 0), parent=play,
        gameplay={"kind": "checkpoint", "size": [10, 2, 4], "race": "course", "order": 1},
    )
    w.add(
        "Halfway", "marker-checkpoint", (hops[6][0], hops[6][1], hops[6][2]), parent=play,
        gameplay={"kind": "checkpoint", "size": [3, 3, 4], "race": "course", "order": 2},
    )
    w.add(
        "Finish", "marker-checkpoint", (0, finish_y, z), parent=play,
        gameplay={"kind": "checkpoint", "size": [8, 8, 4], "race": "course", "order": 3},
    )
    w.add(
        "Welcome", "marker-trigger", (0, 0, 0), parent=play,
        gameplay={
            "kind": "trigger",
            "size": [12, 12, 4],
            "once": True,
            "on_enter": [message("Jump from platform to platform to the gold finish. Don't touch the lava!", 6)],
        },
    )
    del start
    w.write(
        ROOT / "obstacle-course",
        {
            "id": "obstacle-course",
            "name": "Obstacle Course",
            "version": "1.0.0",
            "license": "CC0-1.0",
            "author": "SARE contributors",
            "description": "Jump across floating platforms to the finish. Fall into the lava and you start again; "
            "checkpoints time your run. A template: add your own platforms.",
            "world": {"layout": "layout.json", "spawn": [0, -2, 0]},
        },
        {"hour": 15, "weather": "clear", "scripts": [{"name": "Course results", "source": RACE_RESULTS}]},
    )


def racing_track():
    w = Layout()
    scenery = w.add("Scenery")
    # A 192 m field of grass plates.
    for gx in range(-3, 3):
        for gy in range(-3, 3):
            w.add("Field", "grass", (gx * 32 + 16, gy * 32 + 16, 0), parent=scenery)
    # The track: an oval loop of road points.
    points = []
    for i in range(16):
        a = i / 16 * 2 * math.pi
        points.append([round(70 * math.cos(a), 2), round(40 * math.sin(a), 2), 0.0])
    w.add("Track", "spline-road", (0, 0, 0.02), parent=scenery, road={"points": points, "width": 12, "closed": True})
    for i, (tx, ty) in enumerate([(0, 0), (-30, 0), (30, 0), (0, 70), (0, -70)]):
        w.add("Tree", "tree", (tx, ty, 0), parent=scenery)
    w.add("Stand", "platform", (0, -54, 0), parent=scenery)
    for x in (-20, 20):
        w.add("Lamp post", "lamp-post", (x, -48, 0), parent=scenery,
              light={"color": [1.0, 0.85, 0.6], "size": 1.0, "radius": 8.0, "offset": [0.0, 1.0, 4.25]})

    play = w.add("Gameplay")
    w.add("Start", "marker-spawn", (0, -48, 0), turn=0, parent=play, gameplay={"kind": "spawn"})
    w.add(
        "How to race", "marker-trigger", (0, -48, 0), parent=play,
        gameplay={
            "kind": "trigger",
            "size": [10, 10, 4],
            "once": True,
            "on_enter": [message("Press F9 for your car, then drive through the checkpoints around the track.", 7)],
        },
    )
    for order, i in enumerate([12, 0, 4, 8], start=1):
        x, y, _ = points[i]
        w.add(
            "Start line" if order == 1 else f"Checkpoint {order}", "marker-checkpoint", (x, y, 0), parent=play,
            gameplay={"kind": "checkpoint", "size": [14, 14, 5], "race": "track", "order": order},
        )
    # The last checkpoint sits just before the start line: one lap.
    x, y, _ = points[11]
    w.add(
        "Finish", "marker-checkpoint", (x, y, 0), parent=play,
        gameplay={"kind": "checkpoint", "size": [14, 14, 5], "race": "track", "order": 5},
    )
    w.write(
        ROOT / "racing-track",
        {
            "id": "racing-track",
            "name": "Racing Track",
            "version": "1.0.0",
            "license": "CC0-1.0",
            "author": "SARE contributors",
            "description": "An oval road track on a big field with timed checkpoints for cars. "
            "A template: drag the road's points to make your own track.",
            "world": {"layout": "layout.json", "spawn": [0, -48, 0]},
        },
        {"hour": 17, "weather": "clear", "scripts": [{"name": "Race results", "source": RACE_RESULTS}]},
    )


obstacle_course()
racing_track()
