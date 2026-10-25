"""Reference driver: run the validated Python GTA kernel over fixed scenarios.

Writes one JSONL per scenario (see `scenarios()`) so the Rust port can be
diffed against it tick by tick. Nothing here depends on a map, a renderer or a
recording: the scenarios are a flat ground plane, a seed pose and a control
schedule, so any difference is physics, not collision geometry.
"""
from __future__ import annotations

import json
import math
import os
import sys
from dataclasses import replace
from pathlib import Path

ROOT = Path(os.environ.get("INFERNUS_ROOT", "/mnt/c/Users/berna/mta-sa-dm-standalone"))
sys.path.insert(0, str(ROOT / "infernus-physics" / "src"))

from infernus_physics.gta_derived import (  # noqa: E402
    GtaBodyState,
    GtaControls,
    GtaHandling,
    GtaVehicleState,
    GtaWheelContact,
    initialize_grounded_transmission,
)
from infernus_physics.mta_vehicle import step_player_vehicle  # noqa: E402

CFG = Path(os.environ.get("GTA_CONFIG", "/mnt/f/GTA San Andreas (Reversed)/data/handling.cfg"))
OUT = Path(__file__).resolve().parent / "out"
CTIMER = 1.0  # 50 Hz: one CTimer step per tick


def wheel_definitions(h: GtaHandling) -> list[tuple[float, float]]:
    """Native wheel order used by the reference: FL, RL, FR, RR."""
    half = h.wheel_track / 2.0
    return [
        (-half, h.wheel_front_y),
        (-half, h.wheel_rear_y),
        (half, h.wheel_front_y),
        (half, h.wheel_rear_y),
    ]


def ground_contacts(state) -> list[GtaWheelContact]:
    """Four suspension lines against an infinite plane at z = 0.

    The line runs from the wheel's mount along the body's own down axis, so a
    pitched or rolled car still finds the plane. Identical rule on the Rust
    side: parameterise world_start -> world_end, solve z(lambda) = 0, report the
    model-relative hit point and lambda as the line fraction.
    """
    body = state.body
    h = state.handling
    result: list[GtaWheelContact] = []
    for index, (x, y) in enumerate(wheel_definitions(h)):
        start, end = h.suspension_line(x, y)
        world_start = [
            float(body.position[k]) + sum(float(start[j]) * float(body.basis[j][k]) for j in range(3))
            for k in range(3)
        ]
        world_end = [
            float(body.position[k]) + sum(float(end[j]) * float(body.basis[j][k]) for j in range(3))
            for k in range(3)
        ]
        drop = world_start[2] - world_end[2]
        fraction = (world_start[2] - 0.0) / drop if drop > 1.0e-6 else -1.0
        if fraction < 0.0 or fraction > 1.0:
            delta = [world_end[k] - float(body.position[k]) for k in range(3)]
            local_end = [sum(float(body.basis[j][k]) * delta[k] for k in range(3)) for j in range(3)]
            result.append(GtaWheelContact(index=index, point=tuple(local_end), line_fraction=1.0,
                                          geometric_hit=False, surface_adhesive_limit=4.5))
            continue
        hit = [world_start[k] + fraction * (world_end[k] - world_start[k]) for k in range(3)]
        delta = [hit[k] - float(body.position[k]) for k in range(3)]
        local = [sum(float(body.basis[j][k]) * delta[k] for k in range(3)) for j in range(3)]
        result.append(GtaWheelContact(index=index, point=tuple(local), normal=(0.0, 0.0, 1.0),
                                      line_fraction=fraction, geometric_hit=True,
                                      surface_adhesive_limit=4.5))
    return result


def scenarios() -> list[dict]:
    """Seed + control schedule per scenario, in metres and raw GTA units."""
    out = []

    def add(name, ticks, seed_z, seed_speed, seed_turn, schedule, ground=True):
        controls = [schedule(t) for t in range(ticks)]
        out.append({
            "name": name,
            "ticks": ticks,
            "ctimer_step": CTIMER,
            "ground_z": 0.0 if ground else -1.0e9,
            "seed": {
                "position": [0.0, 0.0, seed_z],
                "basis": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                "move_speed": list(seed_speed),
                "turn_speed": list(seed_turn),
            },
            "controls": controls,
        })

    def idle(_t):
        return {"accelerate": 0.0, "brake_reverse": 0.0, "steering": 0.0, "handbrake": 0.0}

    add("settle", 200, 0.55, (0.0, 0.0, 0.0), (0.0, 0.0, 0.0), idle)
    add("accel", 900, 0.55, (0.0, 0.0, 0.0), (0.0, 0.0, 0.0),
        lambda t: {"accelerate": 1.0, "brake_reverse": 0.0, "steering": 0.0, "handbrake": 0.0})
    add("brake", 300, 0.55, (0.0, 0.6, 0.0), (0.0, 0.0, 0.0),
        lambda t: {"accelerate": 0.0, "brake_reverse": 1.0, "steering": 0.0, "handbrake": 0.0})
    add("corner", 600, 0.55, (0.0, 0.2, 0.0), (0.0, 0.0, 0.0),
        lambda t: {"accelerate": 1.0 if t < 200 else 0.0, "brake_reverse": 0.0,
                   "steering": 0.25, "handbrake": 0.0})
    add("slalom", 600, 0.55, (0.0, 0.3, 0.0), (0.0, 0.0, 0.0),
        lambda t: {"accelerate": 0.6, "brake_reverse": 0.0,
                   "steering": 0.4 if (t // 45) % 2 == 0 else -0.4, "handbrake": 0.0})
    add("handbrake", 300, 0.55, (0.0, 0.4, 0.0), (0.0, 0.0, 0.0),
        lambda t: {"accelerate": 0.0, "brake_reverse": 0.0,
                   "steering": 0.5 if t < 120 else 0.0, "handbrake": 1.0 if t < 120 else 0.0})
    add("rollover", 600, 0.55, (0.0, 0.2, 0.0), (0.0, 0.0, 0.0),
        lambda t: {"accelerate": 1.0, "brake_reverse": 0.0, "steering": 0.6, "handbrake": 0.0})
    add("air", 100, 50.0, (0.0, 0.6, 0.2), (0.0, 0.0, 0.0), idle, ground=False)
    add("reverse", 400, 0.55, (0.0, 0.0, 0.0), (0.0, 0.0, 0.0),
        lambda t: {"accelerate": 0.0, "brake_reverse": 1.0, "steering": 0.0, "handbrake": 0.0})
    return out


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    handling = GtaHandling.from_handling_cfg(CFG, "INFERNUS", model_id=411)
    for scenario in scenarios():
        seed = scenario["seed"]
        state = GtaVehicleState(
            GtaBodyState(
                position=tuple(float(v) for v in seed["position"]),
                basis=tuple(tuple(float(v) for v in row) for row in seed["basis"]),
                move_speed=tuple(float(v) for v in seed["move_speed"]),
                turn_speed=tuple(float(v) for v in seed["turn_speed"]),
            ),
            handling=handling,
        )
        # A spawned race car sits on its wheels through map load and countdown,
        # so its transmission history has settled before the first input. This
        # is the reference's own spawn policy (client_physics_runtime.make_state).
        initialize_grounded_transmission(state)
        (OUT / f"{scenario['name']}.scenario.json").write_text(json.dumps(scenario))
        seed = scenario["seed"]
        rows = [
            f"name {scenario['name']}",
            f"ticks {scenario['ticks']}",
            f"ctimer {scenario['ctimer_step']}",
            f"ground {1 if scenario['ground_z'] > -1.0e8 else 0}",
            "pos " + " ".join(str(v) for v in seed["position"]),
            "basis " + " ".join(str(v) for row in seed["basis"] for v in row),
            "move " + " ".join(str(v) for v in seed["move_speed"]),
            "turn " + " ".join(str(v) for v in seed["turn_speed"]),
        ]
        for control in scenario["controls"]:
            rows.append(f"c {control['accelerate']} {control['brake_reverse']} {control['steering']} {control['handbrake']}")
        (OUT / f"{scenario['name']}.scenario.txt").write_text("\n".join(rows) + "\n")
        lines = []
        for tick, control in enumerate(scenario["controls"], start=1):
            steering = float(control["steering"])
            controls = GtaControls(
                accelerate=float(control["accelerate"]),
                brake_reverse=float(control["brake_reverse"]),
                vehicle_left=max(steering, 0.0),
                vehicle_right=max(-steering, 0.0),
                handbrake=float(control["handbrake"]),
                steer_back=0.0,
                steer_forward=0.0,
                nitro=False,
            )
            contacts = ground_contacts(state) if scenario["ground_z"] > -1.0e8 else [
                GtaWheelContact(index=i, point=(0.0, 0.0, 0.0), line_fraction=1.0, geometric_hit=False)
                for i in range(4)
            ]
            step_player_vehicle(state, controls, contacts, ctimer_step=CTIMER)
            body = state.body
            lines.append(json.dumps({
                "tick": tick,
                "position": [float(v) for v in body.position],
                "basis": [[float(v) for v in row] for row in body.basis],
                "move_speed": [float(v) for v in body.move_speed],
                "turn_speed": [float(v) for v in body.turn_speed],
                "gear": int(getattr(state.transmission, "current_gear", 0)),
                "compressions": [float(v) for v in state.previous_compressions],
                "wheel_counts": [float(v) for v in state.wheel_counts],
                "front_steer": float(getattr(state, "front_steer_angle", 0.0)),
            }))
        (OUT / f"{scenario['name']}.py.jsonl").write_text("\n".join(lines) + "\n")
        print(f"{scenario['name']:10} {scenario['ticks']:5d} ticks  final "
              f"p=({body.position[0]:8.2f},{body.position[1]:9.2f},{body.position[2]:6.2f}) "
              f"v={math.sqrt(sum(float(v)**2 for v in body.move_speed))*50.0*3.6:6.1f} km/h "
              f"gear={getattr(state.transmission, 'current_gear', 0)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
