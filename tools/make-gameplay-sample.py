"""Write content/gameplay-sample: a small Build Mode world that shows every
gameplay behaviour, built only from SARE's own built-in shapes.

    python tools/make-gameplay-sample.py

Coordinates are Blender axes (X east, Y north, Z up, metres); rotation is
degrees. See docs/build-mode.md for the layout format.
"""
import json
import math
import pathlib

OUT = pathlib.Path(__file__).resolve().parent.parent / "content" / "gameplay-sample"
nodes = []


LAMP = {"color": [1.0, 0.85, 0.6], "size": 1.0, "radius": 6.0, "offset": [0.0, 1.0, 4.25]}


def add(name, asset=None, at=(0, 0, 0), turn=0.0, parent=None, gameplay=None, scale=None, light=None):
    node = {"id": len(nodes) + 1, "name": name}
    if parent:
        node["parent"] = parent
    if asset:
        node["asset"] = f"builtin:{asset}"
        node["transform"] = {
            "position": list(at),
            "rotation": [0, 0, turn],
            "scale": list(scale or (1, 1, 1)),
        }
    if gameplay:
        node["gameplay"] = gameplay
    if light:
        node["light"] = light
    nodes.append(node)
    return node["id"]


def message(text, seconds=4):
    return {"do": "message", "text": text, "seconds": seconds}


# The square: two ground plates, a house you can enter, a door that opens.
square = add("Town square")
add("Ground", "ground", (0, 0, 0), parent=square)
add("Course ground", "grass", (0, -32, 0), parent=square)
add("House", "house", (0, 8, 0), parent=square)
for x in (-6, 6):
    add("Tree", "tree", (x, 12, 0), parent=square)
add("Bench", "bench", (4, -2, 0), turn=180, parent=square)
add("Lamp post", "lamp-post", (-4, -2, 0), parent=square, light=LAMP)
add("Lamp post", "lamp-post", (6, 2, 0), turn=180, parent=square, light=LAMP)
add("Lamp post", "lamp-post", (-6, -14, 0), parent=square, light=dict(LAMP, color=[0.6, 0.8, 1.0]))
add("Wall north", "wall", (-10, 3, 0), turn=90, parent=square)
add("Wall south", "wall", (-10, -3, 0), turn=90, parent=square)

# A sculpted hill west of the square: Build Mode terrain, heights in the layout.
CELLS, SIZE = 24, 48.0
heights = []
for row in range(CELLS + 1):
    for column in range(CELLS + 1):
        x = -SIZE / 2 + column * SIZE / CELLS
        y = -SIZE / 2 + row * SIZE / CELLS
        d2 = (x * x + y * y) / (14.0 * 14.0)
        heights.append(round(6.0 * math.exp(-d2), 3))
hill = add("Hill", "terrain", (-44, 0, -0.05), parent=square)
nodes[-1]["terrain"] = {"size": SIZE, "cells": CELLS, "heights": heights, "color": [0.36, 0.5, 0.27]}

play = add("Gameplay")
add("Start", "marker-spawn", (0, -6, 0), parent=play, gameplay={"kind": "spawn"})
outside = add("Outside the house", "marker-target", (0, 2, 0), turn=180, parent=play)
inside = add("Inside the house", "marker-target", (60, -2, 0), parent=play)
add(
    "Front door",
    "door-teleport",
    (0, 4.9, 0),
    parent=play,
    gameplay={
        "kind": "interact",
        "size": [2.4, 3.0, 2.6],
        "prompt": "Go inside",
        "actions": [{"do": "teleport", "target": inside}, message("Inside the house")],
    },
)
add(
    "Garden gate",
    "door",
    (-10, 0, 0),
    turn=90,
    parent=play,
    gameplay={
        "kind": "interact",
        "size": [2.4, 3.0, 2.6],
        "prompt": "Open / close the gate",
        "actions": [{"do": "toggle"}],
    },
)
add(
    "Welcome sign",
    "marker-interact",
    (6, -6, 0),
    parent=play,
    gameplay={
        "kind": "interact",
        "size": [2.0, 2.0, 2.5],
        "prompt": "Read the sign",
        "actions": [
            message(
                "Welcome! Walk into the orange zones, open the gate, enter the house "
                "and run the checkpoint course to the south.",
                8,
            )
        ],
    },
)
add(
    "Night zone",
    "marker-trigger",
    (10, -10, 0),
    parent=play,
    gameplay={
        "kind": "trigger",
        "size": [5, 5, 3],
        "on_enter": [{"do": "set_time", "hour": 22}, message("Night falls")],
        "on_leave": [{"do": "set_time", "hour": 12}],
    },
)
add(
    "Rain zone",
    "marker-trigger",
    (-10, -10, 0),
    parent=play,
    gameplay={
        "kind": "trigger",
        "size": [5, 5, 3],
        "on_enter": [{"do": "set_weather", "weather": "rain"}, message("Here comes the rain")],
        "on_leave": [{"do": "set_weather", "weather": "clear"}],
    },
)
course = add("Checkpoint course", parent=play)
for order, at in enumerate([(-8, -22, 0), (8, -32, 0), (-8, -42, 0)], start=1):
    add(
        f"Checkpoint {order}",
        "marker-checkpoint",
        at,
        parent=course,
        gameplay={"kind": "checkpoint", "size": [6, 6, 4], "race": "main", "order": order},
    )

# The house inside: its own floor far away, reached through the front door.
room = add("Inside")
add("Floor", "ground", (60, 0, 0), parent=room)
for i, x in enumerate((-3, 0, 3)):
    add("Crate", "crate", (60 + x, 6, 0), turn=i * 20, parent=room)
add(
    "Way out",
    "marker-teleport",
    (60, -8, 0),
    parent=room,
    gameplay={
        "kind": "trigger",
        "size": [1.5, 1.5, 2.5],
        "on_enter": [{"do": "teleport", "target": outside}],
    },
)

# World scripts: a bell whose script makes a crate vanish for a moment, and
# a world script that announces race times.
RING = """-- Press E at the bell: the signal crate vanishes for 3 seconds, and
-- everyone sees how often the bell has rung.
local crate = world.find("Signal crate")
local rings = 0

script.object:on('use', function(player)
  rings = rings + 1
  world.hud('bell', 'Bell rung ' .. rings .. (rings == 1 and ' time' or ' times'))
  script.object.color = rgb(1, 0.8, 0.2)
  crate:hide()
  world.message(player.name .. ' rang the bell')
  wait(3)
  crate:show()
end)
"""
RACE = """-- Tells everyone each finish time of the checkpoint course.
local best

world.on('finish', function(player, race, seconds)
  local text = string.format('%s ran the course in %.1f s', player.name, seconds)
  if not best or seconds < best then
    best = seconds
    text = text .. ' - a new best!'
  end
  world.message(text, 6)
end)
"""
add(
    "Bell",
    "marker-interact",
    (-4, -8, 0),
    parent=play,
    gameplay={"kind": "interact", "size": [2.0, 2.0, 2.5], "prompt": "Ring the bell"},
)
nodes[-1]["scripts"] = [{"name": "Ring", "source": RING}]
add("Signal crate", "crate", (-6, -8, 0), parent=play)
LIFT = """-- The lift rises 4 m and comes back down, again and again.
-- Stand on it to ride along.
local lift = script.object
local bottom = lift.position

while true do
  lift:moveTo(bottom + vector(0, 0, 4), 3)
  wait(5)
  lift:moveTo(bottom, 3)
  wait(5)
end
"""
add("Lift", "platform", (16, 6, 0), parent=play)
nodes[-1]["scripts"] = [{"name": "Up and down", "source": LIFT}]

layout = {
    "format": "sare-world-layout",
    "version": 1,
    "next_id": len(nodes) + 1,
    "settings": {
        "hour": 12,
        "weather": "clear",
        "scripts": [{"name": "Race results", "source": RACE}],
    },
    "nodes": nodes,
}
manifest = {
    "id": "gameplay-sample",
    "name": "Gameplay Sample",
    "version": "1.0.0",
    "license": "CC0-1.0",
    "author": "SARE contributors",
    "description": "Every Build Mode gameplay behaviour in one small world: doors, teleports, "
    "trigger zones, time and weather, a checkpoint course and world scripts. Made by "
    "tools/make-gameplay-sample.py.",
    "world": {"layout": "layout.json", "spawn": [0, -6, 0]},
}
OUT.mkdir(parents=True, exist_ok=True)
(OUT / "layout.json").write_text(json.dumps(layout, indent=2) + "\n", encoding="utf-8")
(OUT / "content.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
print(f"wrote {OUT} ({len(nodes)} nodes)")
