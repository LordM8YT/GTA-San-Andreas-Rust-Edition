"""Build the SARE RP Town content pack in Blender.

Run headless from the repository root:

    blender -b --factory-startup --python tools/blender/make_rp_town.py

Options after `--`:
    --out PATH       the .glb to write (default content/sare-rp-town/world.glb)
    --blend PATH     also save the editable .blend file
    --render PATH    render a preview PNG (Cycles, CPU)
    --map PATH       render a top-down map PNG
    --closeup PATH   render a street-level view of the police station
    --lua PATH       write the server locations file
                     (default server-data/resources/[sare-rp]/sare_rp_core/shared/locations.lua;
                     the spawn points go to ../../sare-rp-map/map.lua beside it)
    --no-export      only build the scene (open it in Blender to edit)

Everything is built from code: boxes, cylinders, text and procedural
textures. No third-party assets, so the pack is CC0 like the script.

Coordinates are Blender's: X = east, Y = north, Z = up, in metres. The pack
origin (0, 0, 0) sits at San Andreas world (2500, -1670, 0), so server
scripts use world = (2500 + x, -1670 + y, z). LOCATIONS below is written to
the server's locations.lua so the map and the scripts always agree.
"""
import bpy
import bmesh
import math
import os
import random
import sys

import numpy as np

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
ANCHOR = (2500.0, -1670.0)

# --- Layout -----------------------------------------------------------------
EDGE = 170.0            # the world is 340 x 340 m, walled at +-EDGE
ROADS = (-110.0, 0.0, 110.0)
HALF_ROAD = 6.0
ROAD_TOP = 0.05
BLOCK_TOP = 0.12        # sidewalks and lots: a 7 cm curb above the road
SIDEWALK = 4.0

# Interaction points used by the server scripts: [east, north, up, heading].
LOCATIONS = {
    'spawns': [(-55, -42, 0.2, 180), (-42, -55, 0.2, 90), (-68, -55, 0.2, 270), (-55, -68, 0.2, 0)],
    'police_duty': (45, 67, 0.2, 180),
    'police_armory': (28, 88, 0.2, 0),
    'police_garage': (30, 30, 0.2, 90),
    'jail_cell': (65, 91.5, 0.2, 180),
    'jail_release': (45, 54, 0.2, 180),
    'hospital_checkin': (-62.5, 64, 0.2, 180),
    'hospital_duty': (-84, 84, 0.2, 0),
    'hospital_respawn': (-62.5, 58, 0.2, 180),
    'hospital_garage': (-45, 30, 0.2, 90),
    'bank': (34, -30, 0.2, 270),
    'atms': [(12.5, -50, 0.2, 270), (-50, 53, 0.2, 180), (152, -124.5, 0.2, 0), (8, 121.2, 0.2, 180)],
    'shop_247': (77.5, -38, 0.2, 0),
    'shop_gas': (150, -129, 0.2, 0),
    'city_hall': (55, 138, 0.2, 180),
    'dealer': (132, 60, 0.2, 90),
    'dealer_delivery': (142, 22, 0.2, 0),
    'mechanic': (140, -42, 0.2, 90),
    'mechanic_duty': (158, -30, 0.2, 270),
    'garage': (22, -124, 0.2, 90),
    'garage_spawn': (50, -140, 0.2, 90),
    'taxi_depot': (-30, 125, 0.2, 180),
}

# --- Small helpers ----------------------------------------------------------
rng = random.Random(7)


def args():
    argv = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []
    opts = {'out': os.path.join(ROOT, 'content/sare-rp-town/world.glb'),
            'lua': os.path.join(ROOT, 'server-data/resources/[sare-rp]/sare_rp_core/shared/locations.lua'),
            'blend': None, 'render': None, 'map': None, 'closeup': None, 'export': True}
    i = 0
    while i < len(argv):
        key = argv[i]
        if key == '--no-export':
            opts['export'] = False
            i += 1
            continue
        opts[key.lstrip('-')] = os.path.abspath(argv[i + 1])
        i += 2
    return opts


def hexcol(h):
    h = h.lstrip('#')
    return tuple(int(h[i:i + 2], 16) / 255 for i in (0, 2, 4)) + (1.0,)


# Colours are display values: SARE multiplies them into the texture as they are.
C = {
    'white': hexcol('f2f2ee'), 'offwhite': hexcol('e3ddd0'), 'grey': hexcol('9a9a98'),
    'dark': hexcol('3a3c40'), 'black': hexcol('1c1d20'), 'road_line': hexcol('f4f1e6'),
    'yellow': hexcol('f2c230'), 'police': hexcol('2d4f8f'), 'police_light': hexcol('c9d4e6'),
    'red': hexcol('c8322d'), 'brick': hexcol('a65a42'), 'sand': hexcol('d9c39a'),
    'green': hexcol('4f8a3c'), 'leaf': hexcol('3f7a34'), 'leaf2': hexcol('5a9440'),
    'trunk': hexcol('6b4a2f'), 'gold': hexcol('d4a63a'), 'teal': hexcol('3a9aa0'),
    'orange': hexcol('e07b2c'), 'purple': hexcol('7e5aa6'), 'water': hexcol('5aa0d0'),
    'wood': hexcol('8a6240'), 'metal': hexcol('b8bcc0'), 'glass': (0.55, 0.75, 0.85, 0.35),
}


class Part:
    """One mesh: a material and a collision role (``''``, ``NOCOL_``, ``COL_``)."""

    def __init__(self, name, material):
        self.name, self.material = name, material
        self.verts, self.faces, self.uvs, self.colors = [], [], [], []

    def face(self, pts, color, uv_scale=1.0, uv_offset=(0.0, 0.0)):
        """A polygon, counter-clockwise seen from the outside."""
        a, b, c = (np.array(p, dtype=float) for p in pts[:3])
        n = np.cross(b - a, c - a)
        n = n / (np.linalg.norm(n) or 1.0)
        base = len(self.verts)
        uv = []
        for p in pts:
            if abs(n[2]) > 0.7:
                u, v = p[0], p[1]
            elif abs(n[0]) >= abs(n[1]):
                u, v = p[1] * (1 if n[0] > 0 else -1), p[2]
            else:
                u, v = p[0] * (-1 if n[1] > 0 else 1), p[2]
            uv.append(((u + uv_offset[0]) / uv_scale, (v + uv_offset[1]) / uv_scale))
        self.verts.extend(tuple(p) for p in pts)
        self.faces.append(tuple(range(base, base + len(pts))))
        self.uvs.append(uv)
        self.colors.append(color)

    def box(self, lo, hi, color, uv_scale=1.0, bottom=False, top=True):
        (x0, y0, z0), (x1, y1, z1) = lo, hi
        if x1 <= x0 or y1 <= y0 or z1 <= z0:
            return
        if top:
            self.face([(x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1)], color, uv_scale)
        if bottom:
            self.face([(x0, y1, z0), (x1, y1, z0), (x1, y0, z0), (x0, y0, z0)], color, uv_scale)
        self.face([(x1, y0, z0), (x1, y1, z0), (x1, y1, z1), (x1, y0, z1)], color, uv_scale)
        self.face([(x0, y1, z0), (x0, y0, z0), (x0, y0, z1), (x0, y1, z1)], color, uv_scale)
        self.face([(x1, y1, z0), (x0, y1, z0), (x0, y1, z1), (x1, y1, z1)], color, uv_scale)
        self.face([(x0, y0, z0), (x1, y0, z0), (x1, y0, z1), (x0, y0, z1)], color, uv_scale)

    def cylinder(self, center, radius, z0, z1, color, sides=8, top=True):
        cx, cy = center
        ring = [(cx + radius * math.cos(i / sides * math.tau), cy + radius * math.sin(i / sides * math.tau))
                for i in range(sides)]
        for i in range(sides):
            (ax, ay), (bx, by) = ring[i], ring[(i + 1) % sides]
            self.face([(ax, ay, z0), (bx, by, z0), (bx, by, z1), (ax, ay, z1)], color)
        if top:
            self.face([(x, y, z1) for x, y in ring], color)

    def prism_roof(self, x0, y0, x1, y1, z0, rise, color):
        """A pitched roof with its ridge running east-west."""
        ym = (y0 + y1) / 2
        z1 = z0 + rise
        self.face([(x0, y0, z0), (x1, y0, z0), (x1, ym, z1), (x0, ym, z1)], color)
        self.face([(x1, y1, z0), (x0, y1, z0), (x0, ym, z1), (x1, ym, z1)], color)
        self.face([(x1, y0, z0), (x1, y1, z0), (x1, ym, z1)], color)
        self.face([(x0, y1, z0), (x0, y0, z0), (x0, ym, z1)], color)

    def blob(self, center, radius, z, color):
        """A low-poly tree canopy: an octahedron stretched a little."""
        cx, cy = center
        top, bottom = (cx, cy, z + radius * 1.1), (cx, cy, z - radius * 0.6)
        ring = [(cx + radius * math.cos(i / 6 * math.tau), cy + radius * math.sin(i / 6 * math.tau), z)
                for i in range(6)]
        for i in range(6):
            a, b = ring[i], ring[(i + 1) % 6]
            self.face([a, b, top], color)
            self.face([b, a, bottom], color)


PARTS = {}


def part(material, role=''):
    key = (material, role)
    if key not in PARTS:
        PARTS[key] = Part(f'{role}{material.capitalize()}', material)
    return PARTS[key]


asphalt = lambda: part('asphalt')
concrete = lambda: part('concrete')
grass = lambda: part('grass')
facade = lambda: part('facade')
plain = lambda: part('plain')
deco = lambda: part('plain', 'NOCOL_')
glass = lambda: part('glass', 'NOCOL_')
invisible = lambda: part('plain', 'COL_')

TEXTS = []  # (text, position, rotation_z, size, colour name)


def sign(text, pos, facing, size=1.2, color='white'):
    """Raised letters on a wall. `facing` is the direction the text faces."""
    rot = {'south': 0.0, 'west': -math.pi / 2, 'north': math.pi, 'east': math.pi / 2}[facing]
    TEXTS.append((text, pos, rot, size, color))


# --- Ground, roads and blocks -----------------------------------------------

def cells():
    edges = [-EDGE] + [v for r in ROADS for v in (r - HALF_ROAD, r + HALF_ROAD)] + [EDGE]
    return [(edges[i], edges[i + 1]) for i in range(0, len(edges), 2)]


def build_ground():
    grass().face([(-EDGE - 30, -EDGE - 30, 0), (EDGE + 30, -EDGE - 30, 0),
                  (EDGE + 30, EDGE + 30, 0), (-EDGE - 30, EDGE + 30, 0)], C['white'], 6.0)
    road = asphalt()
    for x in ROADS:
        road.box((x - HALF_ROAD, -EDGE, 0), (x + HALF_ROAD, EDGE, ROAD_TOP), C['white'], 4.0)
    spans = cells()
    for y in ROADS:
        for x0, x1 in spans:
            road.box((x0, y - HALF_ROAD, 0), (x1, y + HALF_ROAD, ROAD_TOP), C['white'], 4.0)
    # Lane markings: dashed yellow centre lines and zebra crossings.
    line = deco()
    z0, z1 = ROAD_TOP, ROAD_TOP + 0.012
    for r in ROADS:
        for x0, x1 in spans:
            t = x0 + 2
            while t + 3 < x1 - 2:
                line.box((t, r - 0.12, z0), (t + 3, r + 0.12, z1), C['yellow'])
                line.box((r - 0.12, t, z0), (r + 0.12, t + 3, z1), C['yellow'])
                t += 6
    for rx in ROADS:
        for ry in ROADS:
            for k in range(-5, 6):
                o = k * 1.0
                # Crossings on all four sides of the junction.
                line.box((rx + o - 0.3, ry + HALF_ROAD + 1, z0), (rx + o + 0.3, ry + HALF_ROAD + 3.5, z1), C['road_line'])
                line.box((rx + o - 0.3, ry - HALF_ROAD - 3.5, z0), (rx + o + 0.3, ry - HALF_ROAD - 1, z1), C['road_line'])
                line.box((rx + HALF_ROAD + 1, ry + o - 0.3, z0), (rx + HALF_ROAD + 3.5, ry + o + 0.3, z1), C['road_line'])
                line.box((rx - HALF_ROAD - 3.5, ry + o - 0.3, z0), (rx - HALF_ROAD - 1, ry + o + 0.3, z1), C['road_line'])


def block(x0, x1, y0, y1, surface):
    """A raised block: a sidewalk ring and an inner surface, flush at BLOCK_TOP."""
    s, z = SIDEWALK, BLOCK_TOP
    walk = concrete()
    walk.box((x0, y0, 0), (x1, y0 + s, z), C['white'], 2.0)
    walk.box((x0, y1 - s, 0), (x1, y1, z), C['white'], 2.0)
    walk.box((x0, y0 + s, 0), (x0 + s, y1 - s, z), C['white'], 2.0)
    walk.box((x1 - s, y0 + s, 0), (x1, y1 - s, z), C['white'], 2.0)
    inner = {'grass': grass, 'asphalt': asphalt, 'concrete': concrete}[surface]()
    scale = {'grass': 6.0, 'asphalt': 4.0, 'concrete': 2.0}[surface]
    inner.box((x0 + s, y0 + s, 0), (x1 - s, y1 - s, z), C['white'], scale)
    return (x0 + s, x1 - s, y0 + s, y1 - s)


def build_blocks():
    spans = cells()
    surfaces = {}
    # Inner blocks: police (NE), hospital (NW), bank and shop (SE), park (SW).
    plan = {
        (2, 2): 'asphalt', (1, 2): 'asphalt', (2, 1): 'concrete', (1, 1): 'grass',
        (3, 2): 'asphalt', (3, 1): 'asphalt', (3, 3): 'grass', (3, 0): 'concrete',
        (0, 2): 'grass', (0, 1): 'grass', (0, 3): 'grass', (0, 0): 'concrete',
        (1, 3): 'grass', (2, 3): 'concrete', (1, 0): 'concrete', (2, 0): 'asphalt',
    }
    for i, (x0, x1) in enumerate(spans):
        for j, (y0, y1) in enumerate(spans):
            surfaces[(i, j)] = block(x0, x1, y0, y1, plan[(i, j)])
    return surfaces


# --- Buildings --------------------------------------------------------------
Z = BLOCK_TOP


def solid_building(x0, y0, x1, y1, height, tint, roof=C['grey'], trim=C['offwhite']):
    """A closed tower with window facades and a parapet."""
    facade().box((x0, y0, Z), (x1, y1, Z + height), tint, 3.0, top=False)
    plain().face([(x0, y0, Z + height), (x1, y0, Z + height), (x1, y1, Z + height), (x0, y1, Z + height)], roof)
    p = 0.4
    for lo, hi in [((x0, y0), (x1, y0 + p)), ((x0, y1 - p), (x1, y1)), ((x0, y0), (x0 + p, y1)), ((x1 - p, y0), (x1, y1))]:
        plain().box((lo[0], lo[1], Z + height), (hi[0], hi[1], Z + height + 0.9), trim)
    # A door so it reads as a building from the street.


def door_panel(x, y, facing, width=1.8, color=C['dark']):
    """A dark door rectangle just in front of a solid wall (decoration)."""
    h = 2.4
    d = 0.06
    if facing == 'south':
        deco().box((x - width / 2, y - d, Z), (x + width / 2, y, Z + h), color)
    elif facing == 'north':
        deco().box((x - width / 2, y, Z), (x + width / 2, y + d, Z + h), color)
    elif facing == 'west':
        deco().box((x - d, y - width / 2, Z), (x, y + width / 2, Z + h), color)
    else:
        deco().box((x, y - width / 2, Z), (x + d, y + width / 2, Z + h), color)


def hollow(x0, y0, x1, y1, height, doors, wall, inside=C['offwhite'], floor=C['white'], roof=C['grey'], t=0.35):
    """A building you can walk into. `doors` is a list of (side, centre, width)."""
    door_h = 2.8
    walls = plain()
    for side in ('south', 'north', 'west', 'east'):
        if side in ('south', 'north'):
            y = y0 if side == 'south' else y1 - t
            a, b = x0, x1
        else:
            x = x0 if side == 'west' else x1 - t
            a, b = y0 + t, y1 - t
        gaps = sorted((c - w / 2, c + w / 2) for s, c, w in doors if s == side)
        pieces, cursor = [], a
        for g0, g1 in gaps:
            pieces.append((cursor, g0))
            cursor = g1
        pieces.append((cursor, b))
        for p0, p1 in pieces:
            if side in ('south', 'north'):
                walls.box((p0, y, Z), (p1, y + t, Z + height), wall)
            else:
                walls.box((x, p0, Z), (x + t, p1, Z + height), wall)
        for g0, g1 in gaps:  # lintel over each door
            if side in ('south', 'north'):
                walls.box((g0, y, Z + door_h), (g1, y + t, Z + height), wall)
            else:
                walls.box((x, g0, Z + door_h), (x + t, g1, Z + height), wall)
    # Floor and ceiling/roof slab.
    concrete().box((x0 + t, y0 + t, Z), (x1 - t, y1 - t, Z + 0.04), floor, 2.0)
    plain().box((x0, y0, Z + height), (x1, y1, Z + height + 0.3), roof, bottom=True)
    # A coloured band inside so rooms do not look empty.
    deco().box((x0 + t, y0 + t, Z + 0.04), (x1 - t, y0 + t + 0.02, Z + 1.0), inside)
    deco().box((x0 + t, y1 - t - 0.02, Z + 0.04), (x1 - t, y1 - t, Z + 1.0), inside)


def counter(x0, y0, x1, y1, color=C['wood'], top=C['offwhite']):
    plain().box((x0, y0, Z), (x1, y1, Z + 1.05), color)
    plain().box((x0 - 0.05, y0 - 0.05, Z + 1.05), (x1 + 0.05, y1 + 0.05, Z + 1.12), top)


def marker(x, y, color):
    """A coloured floor pad where a player interacts (walk-through)."""
    deco().cylinder((x, y), 0.9, Z + 0.04, Z + 0.07, color, sides=12)


def lamp_post(x, y, facing):
    plain().cylinder((x, y), 0.12, Z, Z + 6.5, C['dark'], sides=6)
    dx, dy = {'east': (1.6, 0), 'west': (-1.6, 0), 'north': (0, 1.6), 'south': (0, -1.6)}[facing]
    deco().box((min(x, x + dx) - 0.08, min(y, y + dy) - 0.08, Z + 6.3), (max(x, x + dx) + 0.08, max(y, y + dy) + 0.08, Z + 6.5), C['dark'])
    deco().box((x + dx - 0.35, y + dy - 0.35, Z + 6.1), (x + dx + 0.35, y + dy + 0.35, Z + 6.3), C['yellow'])


def tree(x, y, scale=1.0):
    plain().cylinder((x, y), 0.25 * scale, Z, Z + 2.6 * scale, C['trunk'], sides=6)
    deco().blob((x, y), 2.2 * scale, Z + 3.6 * scale, rng.choice([C['leaf'], C['leaf2']]))


def bench(x, y, along_x=True):
    if along_x:
        plain().box((x - 1, y - 0.25, Z + 0.4), (x + 1, y + 0.25, Z + 0.5), C['wood'])
        plain().box((x - 1, y + 0.2, Z + 0.5), (x + 1, y + 0.3, Z + 0.95), C['wood'])
        for dx in (-0.85, 0.85):
            plain().box((x + dx - 0.05, y - 0.2, Z), (x + dx + 0.05, y + 0.2, Z + 0.4), C['dark'])
    else:
        plain().box((x - 0.25, y - 1, Z + 0.4), (x + 0.25, y + 1, Z + 0.5), C['wood'])
        plain().box((x + 0.2, y - 1, Z + 0.5), (x + 0.3, y + 1, Z + 0.95), C['wood'])
        for dy in (-0.85, 0.85):
            plain().box((x - 0.2, y + dy - 0.05, Z), (x + 0.2, y + dy + 0.05, Z + 0.4), C['dark'])


def car(x, y, heading_deg, color):
    """A parked toy car (solid), the same size as SARE Kart."""
    a = math.radians(heading_deg)
    ca, sa = math.cos(a), math.sin(a)

    def rot_box(target, lo, hi, col):
        # Axis-aligned when the heading is a multiple of 90 degrees.
        (lx0, ly0, z0), (lx1, ly1, z1) = lo, hi
        pts = [(lx0 * ca - ly0 * sa, lx0 * sa + ly0 * ca), (lx1 * ca - ly1 * sa, lx1 * sa + ly1 * ca)]
        xs, ys = sorted(p[0] for p in pts), sorted(p[1] for p in pts)
        target.box((x + xs[0], y + ys[0], Z + z0), (x + xs[1], y + ys[1], Z + z1), col)

    rot_box(plain(), (-1.0, -2.0, 0.35), (1.0, 2.0, 1.0), color)
    rot_box(plain(), (-0.85, -0.9, 1.0), (0.85, 1.0, 1.55), C['dark'])
    for wx in (-1.0, 0.9):
        for wy in (-1.4, 1.2):
            rot_box(plain(), (wx, wy, 0.0), (wx + 0.1, wy + 0.2 + 0.4, 0.55), C['black'])


def parking_lines(x0, x1, y, depth, step=3.0, color=C['road_line']):
    t = x0
    while t <= x1:
        deco().box((t - 0.08, y, Z), (t + 0.08, y + depth, Z + 0.012), color)
        t += step


def build_police():
    # LSPD station, walk-in: lobby with front desk, lockers and two cells.
    x0, y0, x1, y1 = 20, 60, 70, 96
    hollow(x0, y0, x1, y1, 9, [('south', 45, 4.0)], C['police_light'], inside=C['police'])
    plain().box((x0 - 0.05, y0 - 0.05, Z + 3.4), (x1 + 0.05, y0 + 0.4, Z + 4.2), C['police'])  # blue band
    plain().box((x0 - 0.05, y1 - 0.4, Z + 3.4), (x1 + 0.05, y1 + 0.05, Z + 4.2), C['police'])
    sign('POLICE', (45, y0 - 0.12, Z + 5.4), 'south', 1.8, 'white')
    counter(39, 69, 51, 70.2)
    marker(45, 67, C['police'])
    # Lockers/armory on the west wall.
    for k in range(6):
        plain().box((20.4, 84 + k * 1.0, Z), (21.2, 84.9 + k * 1.0, Z + 2.1), C['metal'])
    marker(28, 88, C['police'])
    # Cells: bars to the west and south of the north-east corner.
    bars = plain()
    cx0, cy0 = 60.0, 80.0
    t = cx0
    while t < x1 - 0.35:
        bars.box((t, cy0 - 0.04, Z), (t + 0.06, cy0 + 0.04, Z + 3.0), C['dark'])
        t += 0.28
    t = cy0
    while t < y1 - 0.35:
        bars.box((cx0 - 0.04, t, Z), (cx0 + 0.04, t + 0.06, Z + 3.0), C['dark'])
        t += 0.28
    bars.box((cx0 - 0.1, cy0 - 0.1, Z + 3.0), (x1 - 0.35, cy0 + 0.1, Z + 3.2), C['dark'])
    bars.box((cx0 - 0.1, cy0, Z + 3.0), (cx0 + 0.1, y1 - 0.35, Z + 3.2), C['dark'])
    plain().box((61, 93, Z), (64, 95.6, Z + 0.5), C['metal'])  # bunk
    # A wall splitting the cell in two.
    plain().box((cx0, 87.8, Z), (x1 - 0.35, 88.2, Z + 9), C['police_light'])
    # Parking lot with patrol cars and a flag.
    parking_lines(15, 60, 12, 5.0, 3.5)
    for k, px in enumerate((22, 29, 36)):
        car(px + 1.75, 14.5, 0, C['police'] if k != 1 else C['white'])
    marker(30, 30, C['police'])
    plain().cylinder((80, 58), 0.1, Z, Z + 10, C['metal'], sides=6)
    deco().box((80.1, 57.95, Z + 8.2), (82.6, 58.05, Z + 9.8), C['police'])
    lamp_post(11, 40, 'west')
    lamp_post(11, 80, 'west')


def build_hospital():
    x0, y0, x1, y1 = -96, 55, -30, 96
    hollow(x0, y0, x1, y1, 12, [('south', -62.5, 5.0)], C['white'], inside=hexcol('d6e8e6'))
    plain().box((x0 - 0.05, y0 - 0.05, Z + 0.0), (x1 + 0.05, y0 + 0.4, Z + 0.6), C['teal'])
    sign('HOSPITAL', (-62.5, y0 - 0.12, Z + 6.0), 'south', 2.0, 'red')
    # Red cross over the entrance.
    deco().box((-63.2, y0 - 0.25, Z + 8.0), (-61.8, y0 - 0.05, Z + 11.0), C['red'])
    deco().box((-64.0, y0 - 0.25, Z + 8.8), (-61.0, y0 - 0.05, Z + 10.2), C['red'])
    counter(-68, 66, -57, 67.2, C['white'], C['teal'])
    # ATM on the front wall.
    plain().box((-50.6, 54.2, Z), (-49.4, 55.0, Z + 1.8), C['dark'])
    deco().box((-50.3, 54.14, Z + 1.0), (-49.7, 54.2, Z + 1.5), C['teal'])
    marker(-62.5, 64, C['red'])
    marker(-84, 84, C['teal'])
    # Beds along the north wall.
    for k in range(6):
        bx = -90 + k * 4.0
        plain().box((bx, 91.5, Z), (bx + 1.1, 95.4, Z + 0.7), C['white'])
        plain().box((bx + 0.1, 94.6, Z + 0.7), (bx + 1.0, 95.3, Z + 0.85), hexcol('a8d0e0'))
    # Ambulance bay and helipad.
    parking_lines(-55, -35, 14, 6.0, 5.0, C['red'])
    marker(-45, 30, C['red'])
    car(-47.5, 17, 0, C['white'])
    deco().cylinder((-82, 28), 9.0, Z, Z + 0.02, C['dark'], sides=24)
    deco().box((-84.5, 24, Z + 0.02), (-83.5, 32, Z + 0.035), C['yellow'])
    deco().box((-80.5, 24, Z + 0.02), (-79.5, 32, Z + 0.035), C['yellow'])
    deco().box((-83.5, 27.5, Z + 0.02), (-80.5, 28.5, Z + 0.035), C['yellow'])
    lamp_post(-11, 40, 'east')
    lamp_post(-11, 80, 'east')


def build_bank_and_shop():
    # Bank: front on the main street (west), columns and a gold trim.
    x0, y0, x1, y1 = 15, -46, 56, -14
    hollow(x0, y0, x1, y1, 10, [('west', -30, 4.0)], C['sand'], inside=hexcol('e9dcc0'))
    for lo, hi in [((x0 - 0.3, y0 - 0.3), (x1 + 0.3, y0 + 0.5)), ((x0 - 0.3, y1 - 0.5), (x1 + 0.3, y1 + 0.3)),
                   ((x0 - 0.3, y0 + 0.5), (x0 + 0.5, y1 - 0.5)), ((x1 - 0.5, y0 + 0.5), (x1 + 0.3, y1 - 0.5))]:
        plain().box((lo[0], lo[1], Z + 10.3), (hi[0], hi[1], Z + 11.0), C['gold'])
    for px, py in [(20, -60), (30, -60), (40, -60), (50, -60), (70, -60), (80, -60), (90, -60)]:
        plain().box((px - 1, py - 1, Z), (px + 1, py + 1, Z + 0.6), C['grey'])
        deco().blob((px, py), 1.0, Z + 1.2, C['leaf2'])
    for cy in (-40, -35, -25, -20):
        plain().cylinder((x0 - 2.2, cy), 0.55, Z, Z + 10.0, C['offwhite'], sides=10)
    plain().box((x0 - 3.2, y0 + 2, Z + 10.0), (x0, y1 - 2, Z + 10.4), C['offwhite'])
    sign('BANK', (x0 - 0.12, -30, Z + 6.0), 'west', 2.0, 'gold')
    counter(38, -42, 39.2, -18, C['wood'], C['gold'])
    marker(34, -30, C['gold'])
    plain().cylinder((55.6, -30), 2.2, Z + 0.3, Z + 0.6, C['metal'], sides=16)  # vault door
    # ATM outside the bank.
    plain().box((12.8, -50.6, Z), (13.6, -49.4, Z + 1.8), C['dark'])
    deco().box((12.74, -50.3, Z + 1.0), (12.8, -49.7, Z + 1.5), C['teal'])
    # 24/7 shop: door on the north side.
    sx0, sy0, sx1, sy1 = 62, -48, 96, -22
    hollow(sx0, sy0, sx1, sy1, 5.5, [('north', 77.5, 3.5)], C['orange'], inside=C['white'])
    sign('24/7', (77.5, sy1 + 0.12, Z + 3.6), 'north', 1.4, 'white')
    for k in range(4):
        plain().box((65 + k * 7, -44, Z), (70 + k * 7, -43, Z + 1.8), C['metal'])
        for shelf in (0.6, 1.2, 1.8):
            deco().box((65 + k * 7, -44.1, Z + shelf - 0.25), (70 + k * 7, -43.95, Z + shelf), rng.choice([C['red'], C['yellow'], C['teal'], C['green']]))
    counter(72, -36, 83, -35, C['orange'], C['white'])
    marker(77.5, -38, C['orange'])
    # Plaza benches and a parking strip.
    for bx in (25, 35, 45):
        bench(bx, -8.5)
    parking_lines(15, 95, -100, 5.0, 3.0)
    lamp_post(11, -70, 'west')
    lamp_post(60, -60, 'north')


def build_park():
    # Legion Square: paths, a fountain, trees and benches. Players spawn here.
    cx, cy = -55, -55
    path = concrete()
    path.box((-100, cy - 2, Z), (cx - 12, cy + 2, Z + 0.02), C['offwhite'], 2.0)
    path.box((cx + 12, cy - 2, Z), (-10, cy + 2, Z + 0.02), C['offwhite'], 2.0)
    path.box((cx - 2, -100, Z), (cx + 2, cy - 12, Z + 0.02), C['offwhite'], 2.0)
    path.box((cx - 2, cy + 12, Z), (cx + 2, -10, Z + 0.02), C['offwhite'], 2.0)
    path.box((cx - 12, cy - 12, Z), (cx + 12, cy + 12, Z + 0.02), C['offwhite'], 2.0)
    plain().cylinder((cx, cy), 5.0, Z, Z + 0.7, C['offwhite'], sides=16, top=False)
    deco().cylinder((cx, cy), 4.7, Z, Z + 0.55, C['water'], sides=16)
    plain().cylinder((cx, cy), 0.8, Z, Z + 2.2, C['offwhite'], sides=10)
    deco().cylinder((cx, cy), 1.6, Z + 2.2, Z + 2.4, C['offwhite'], sides=12)
    for (tx, ty) in [(-90, -20), (-80, -30), (-30, -20), (-20, -35), (-90, -90), (-75, -80),
                     (-25, -85), (-35, -95), (-90, -55), (-20, -60), (-55, -20), (-55, -90),
                     (-70, -35), (-40, -75)]:
        tree(tx + rng.uniform(-2, 2), ty + rng.uniform(-2, 2), rng.uniform(0.9, 1.25))
    for bx, by, ax in [(-55, -45, True), (-55, -65, True), (-45, -55, False), (-65, -55, False)]:
        bench(bx + (4 if ax else 0), by + (0 if ax else 4), ax)
    for lx, ly in [(-68, -42), (-42, -42), (-68, -68), (-42, -68)]:
        lamp_post(lx, ly, 'north')


def build_east():
    # Car dealer with a glass front on the west road.
    x0, y0, x1, y1 = 122, 40, 160, 82
    hollow(x0, y0, x1, y1, 8, [('west', 61, 4.0)], C['white'], inside=hexcol('cfd8e0'))
    # Replace most of the front wall look with glass: panes in front of it.
    for gy0, gy1 in ((42, 58.5), (63.5, 80)):
        glass().box((x0 - 0.15, gy0, Z + 0.3), (x0 - 0.05, gy1, Z + 6.8), C['white'])
    plain().box((x0 - 0.3, y0, Z + 6.8), (x0, y1, Z + 8.3), C['dark'])
    sign('PREMIUM MOTORS', (x0 - 0.35, 61, Z + 7.15), 'west', 0.9, 'white')
    car(135, 50, 0, C['red'])
    car(148, 72, 90, C['yellow'])
    car(150, 50, 0, C['teal'])
    counter(140, 58, 141.2, 64, C['dark'], C['white'])
    marker(132, 60, C['teal'])
    parking_lines(125, 160, 12, 6.0, 4.0)
    for k, colr in enumerate((C['purple'], C['orange'], C['green'])):
        car(127 + k * 4 + 2, 15, 0, colr)
    marker(142, 22, C['teal'])
    lamp_post(118, 30, 'west')
    # Mechanic garage: two open bays on the west road.
    gx0, gy0, gx1, gy1 = 125, -62, 165, -22
    hollow(gx0, gy0, gx1, gy1, 7.5, [('west', -48, 8.0), ('west', -34, 8.0)], C['dark'], inside=C['yellow'], floor=C['grey'])
    deco().box((gx0 - 0.3, -52.5, Z + 3.4), (gx0 - 0.05, -29.5, Z + 3.7), C['yellow'])
    sign('GARAGE', (gx0 - 0.12, -41, Z + 5.2), 'west', 1.6, 'yellow')
    for by in (-48, -34):
        deco().box((132, by - 2.2, Z + 0.04), (144, by + 2.2, Z + 0.06), C['yellow'])
    marker(140, -42, C['yellow'])
    for k in range(5):
        plain().box((164.2, -36 + k * 1.2, Z), (164.6, -35.2 + k * 1.2, Z + 1.0), C['red'])  # tool chests
    marker(158, -30, C['yellow'])
    car(138, -48, 90, C['grey'])
    # Apartments in the north-east corner and a gas station in the south-east.
    solid_building(124, 122, 150, 150, 32, C['offwhite'])
    door_panel(137, 122, 'south')
    solid_building(152, 130, 166, 160, 20, hexcol('c9b8a0'))
    gas_station()


def gas_station():
    # Canopy over four pumps, a small shop kiosk with an ATM.
    for px in (128, 134, 140, 146):
        for py in (-150, -142):
            pass
    plain().box((124, -156, Z + 5.2), (152, -136, Z + 5.8), C['white'])
    deco().box((123.9, -156.1, Z + 5.2), (152.1, -135.9, Z + 5.5), C['red'])
    for cx, cy in [(126, -154), (150, -154), (126, -138), (150, -138)]:
        plain().box((cx - 0.3, cy - 0.3, Z), (cx + 0.3, cy + 0.3, Z + 5.2), C['white'])
    for px in (131, 138, 145):
        plain().box((px - 0.5, -146.8, Z), (px + 0.5, -145.2, Z + 0.15), C['offwhite'])
        plain().box((px - 0.35, -146.4, Z + 0.15), (px + 0.35, -145.6, Z + 1.7), C['red'])
    sign('GAS', (138, -156.25, Z + 5.35), 'south', 0.8, 'white')
    hollow(144, -128, 162, -118, 4.5, [('south', 150, 2.5)], C['red'], inside=C['white'])
    sign('24/7 GAS', (153, -128.12, Z + 3.4), 'south', 0.9, 'white')
    counter(146, -124, 154, -123, C['red'], C['white'])
    marker(150, -129, C['orange'])
    plain().box((151.4, -126.1, Z), (152.6, -125.3, Z + 1.8), C['dark'])  # ATM on the shop wall
    deco().box((151.7, -125.36, Z + 1.0), (152.3, -125.3, Z + 1.5), C['teal'])
    lamp_post(118, -135, 'west')


def house(x0, y0, x1, y1, wall, roof, door_side):
    plain().box((x0, y0, Z), (x1, y1, Z + 3.6), wall)
    plain().prism_roof(x0 - 0.5, y0 - 0.5, x1 + 0.5, y1 + 0.5, Z + 3.6, 2.4, roof)
    cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
    if door_side == 'east':
        door_panel(x1, cy, 'east', 1.2, C['wood'])
        for oy in (-2.5, 2.5):
            deco().box((x1, cy + oy - 0.8, Z + 1.2), (x1 + 0.05, cy + oy + 0.8, Z + 2.5), hexcol('86b7d4'))
    else:
        door_panel(x0, cy, 'west', 1.2, C['wood'])
        for oy in (-2.5, 2.5):
            deco().box((x0 - 0.05, cy + oy - 0.8, Z + 1.2), (x0, cy + oy + 0.8, Z + 2.5), hexcol('86b7d4'))


def build_west():
    colours = [hexcol('e8d6b0'), hexcol('b7d0c4'), hexcol('e6b9a6'), hexcol('c8c2e0'), hexcol('f0e2a0'), hexcol('cfd6dc')]
    roofs = [C['brick'], hexcol('5b5f66'), hexcol('7a4a3a')]
    k = 0
    for cy in (20, 50, 80, -20, -50, -80):
        house(-150, cy - 6, -138, cy + 6, colours[k % 6], roofs[k % 3], 'east')
        deco().box((-137.9, cy - 0.8, Z), (-120, cy + 0.8, Z + 0.02), C['grey'])  # driveway
        tree(-158, cy + 7, 0.9)
        k += 1
    # Warehouse yard in the south-west corner with containers.
    plain().box((-165, -165, Z), (-130, -140, Z + 9), hexcol('7d8a90'))
    sign('WAREHOUSE', (-147.5, -139.88, Z + 6.5), 'north', 1.4, 'white')
    for k, (cx, cy) in enumerate([(-125, -160), (-125, -155), (-125, -150), (-160, -128), (-153, -128)]):
        plain().box((cx - 6 if k < 3 else cx - 1.2, cy - 1.2 if k < 3 else cy - 6, Z),
                    (cx + 6 if k < 3 else cx + 1.2, cy + 1.2 if k < 3 else cy + 6, Z + 2.6),
                    [C['red'], C['teal'], C['orange'], C['police'], C['green']][k])
    # North-west corner: a small basketball court.
    deco().box((-160, 125, Z), (-126, 158, Z + 0.02), hexcol('c76b3e'))
    for hx in (-158, -128):
        plain().cylinder((hx, 141.5), 0.1, Z, Z + 3.2, C['metal'], sides=6)
        plain().box((hx - 0.05 if hx < 0 else hx - 0.05, 140.6, Z + 3.0), (hx + 0.05, 142.4, Z + 4.0), C['white'])


def build_north():
    # City hall with steps and columns; the taxi depot; apartments.
    x0, y0, x1, y1 = 30, 128, 80, 160
    hollow(x0, y0, x1, y1, 11, [('south', 55, 4.0)], C['offwhite'], inside=hexcol('e4dccb'))
    for k in range(3):
        plain().box((44 - k * 0.0, y0 - 3 + k, Z), (66, y0 - 2 + k, Z + 0.15 * (k + 1)), C['white'])
    for cx in (46, 50, 60, 64):
        plain().cylinder((cx, y0 - 1.0), 0.5, Z + 0.45, Z + 10.0, C['white'], sides=10)
    plain().box((43, y0 - 2.2, Z + 10.0), (67, y0, Z + 11.0), C['white'])
    sign('CITY HALL', (55, y0 - 2.32, Z + 10.3), 'south', 0.8, 'dark')
    counter(48, 141, 62, 142.2, C['wood'], C['white'])
    marker(55, 138, C['green'])
    # ATM by the city hall steps.
    plain().box((7.4, 122.0, Z), (8.6, 122.8, Z + 1.8), C['dark'])
    deco().box((7.7, 121.94, Z + 1.0), (8.3, 122.0, Z + 1.5), C['teal'])
    # Taxi depot.
    plain().box((-55, 128, Z), (-15, 150, Z + 6), C['yellow'])
    sign('TAXI', (-35, 127.88, Z + 4.2), 'south', 1.6, 'dark')
    door_panel(-30, 128, 'south', 3.0)
    marker(-30, 125, C['yellow'])
    for k in range(3):
        car(-50 + k * 5, 158, 0, C['yellow'])
    solid_building(-100, 128, -70, 162, 26, hexcol('b9c4cc'))
    door_panel(-85, 128, 'south')
    for tx in (-62, -8, 90, 100):
        tree(tx, 124, 1.0)


def build_south():
    # Public garage lot and a row of shops.
    marker(22, -124, C['purple'])
    plain().box((14, -122, Z), (18, -118, Z + 2.6), C['purple'])
    sign('PARKING', (16, -117.88, Z + 1.6), 'north', 0.6, 'white')
    parking_lines(30, 95, -165, 5.5, 3.5)
    parking_lines(30, 95, -136, 5.5, 3.5)
    for k, colr in enumerate((C['red'], C['grey'], C['teal'], C['white'])):
        car(36 + k * 14 + 1.75, -162, 0, colr)
    solid_building(-100, -165, -75, -125, 7, hexcol('d98a6a'))
    door_panel(-87.5, -125, 'north', 2.4)
    sign('DINER', (-87.5, -124.88, Z + 4.6), 'north', 1.6, 'white')
    solid_building(-70, -165, -45, -125, 9, hexcol('8fb1c9'))
    door_panel(-57.5, -125, 'north', 2.4)
    sign('CLOTHES', (-57.5, -124.88, Z + 5.6), 'north', 1.3, 'white')
    solid_building(-40, -165, -12, -130, 14, hexcol('c4c0b0'))
    door_panel(-26, -130, 'north', 2.4)


def build_edges():
    # A low visible barrier and an invisible taller wall at the world edge.
    e = EDGE
    for lo, hi in [((-e - 1, -e - 1, 0), (e + 1, -e, 1.1)), ((-e - 1, e, 0), (e + 1, e + 1, 1.1)),
                   ((-e - 1, -e, 0), (-e, e, 1.1)), ((e, -e, 0), (e + 1, e, 1.1))]:
        plain().box(lo, hi, C['grey'])
    for lo, hi in [((-e - 1, -e - 1, 0), (e + 1, -e, 8)), ((-e - 1, e, 0), (e + 1, e + 1, 8)),
                   ((-e - 1, -e, 0), (-e, e, 8)), ((e, -e, 0), (e + 1, e, 8))]:
        invisible().box(lo, hi, C['white'])
    # Street lights along the main roads.
    for r in ROADS:
        t = -150.0
        while t <= 150:
            if all(abs(t - q) > 14 for q in ROADS):
                lamp_post(r + HALF_ROAD + 1.0, t, 'west')
                lamp_post(t, r - HALF_ROAD - 1.0, 'north')
            t += 40.0


# --- Blender scene ----------------------------------------------------------

def texture(name, size, pixel):
    img = bpy.data.images.new(name, size, size, alpha=False)
    data = np.zeros((size, size, 4), dtype=np.float32)
    for y in range(size):
        for x in range(size):
            data[y, x] = pixel(x, y)
    img.pixels.foreach_set(data.ravel())
    img.pack()
    return img


def make_textures():
    noise = random.Random(3)

    def grass_px(x, y):
        v = noise.uniform(-0.05, 0.05)
        return (0.30 + v, 0.52 + v * 1.4, 0.22 + v, 1)

    def asphalt_px(x, y):
        v = noise.uniform(-0.03, 0.03)
        return (0.27 + v, 0.27 + v, 0.29 + v, 1)

    def concrete_px(x, y):
        joint = x % 32 == 0 or y % 32 == 0
        v = noise.uniform(-0.02, 0.02)
        t = 0.55 if joint else 0.74 + v
        return (t, t, t * 0.98, 1)

    def facade_px(x, y):
        # One 3 x 3 m bay: a window with a frame on a light wall.
        wx, wy = 10 <= x < 54, 14 <= y < 52
        frame = (8 <= x < 56 and 12 <= y < 54) and not (wx and wy)
        if wx and wy:
            glint = 0.06 if (x - y) % 23 < 4 else 0.0
            return (0.20 + glint, 0.27 + glint, 0.35 + glint, 1)
        if frame:
            return (0.62, 0.62, 0.62, 1)
        return (0.93, 0.93, 0.92, 1)

    return {
        'grass': texture('grass', 64, grass_px),
        'asphalt': texture('asphalt', 64, asphalt_px),
        'concrete': texture('concrete', 64, concrete_px),
        'facade': texture('facade', 64, facade_px),
    }


def make_material(name, image=None, alpha=None, base=(1, 1, 1, 1)):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    nodes, links = mat.node_tree.nodes, mat.node_tree.links
    bsdf = nodes['Principled BSDF']
    bsdf.inputs['Base Color'].default_value = base
    bsdf.inputs['Roughness'].default_value = 0.85
    if image is not None:
        tex = nodes.new('ShaderNodeTexImage')
        tex.image = image
        tex.interpolation = 'Closest'
        links.new(tex.outputs['Color'], bsdf.inputs['Base Color'])
    if alpha is not None:
        bsdf.inputs['Alpha'].default_value = alpha
        mat.blend_method = 'BLEND'
    return mat


def build_objects(textures):
    mats = {
        'grass': make_material('Grass', textures['grass']),
        'asphalt': make_material('Asphalt', textures['asphalt']),
        'concrete': make_material('Concrete', textures['concrete']),
        'facade': make_material('Facade', textures['facade']),
        'plain': make_material('Plain'),
        'glass': make_material('Glass', alpha=0.35, base=C['glass']),
    }
    coll = bpy.context.scene.collection
    objects = []
    for (material, role), p in PARTS.items():
        mesh = bpy.data.meshes.new(p.name)
        mesh.from_pydata(p.verts, [], p.faces)
        mesh.update()
        uv = mesh.uv_layers.new(name='UVMap')
        uv.data.foreach_set('uv', [c for face in p.uvs for u in face for c in u])
        colors = mesh.color_attributes.new('Color', 'FLOAT_COLOR', 'CORNER')
        colors.data.foreach_set('color', [c for face, col in zip(p.faces, p.colors) for _ in face for c in col])
        mesh.color_attributes.active_color = colors
        mesh.color_attributes.render_color_index = 0
        mesh.materials.append(mats[material])
        obj = bpy.data.objects.new(p.name, mesh)
        coll.objects.link(obj)
        objects.append(obj)
    # Signs: extruded text converted to meshes, drawn but not solid.
    sign_mats = {}
    depsgraph = bpy.context.evaluated_depsgraph_get()
    for i, (text, (x, y, z), rot, size, colour) in enumerate(TEXTS):
        if colour not in sign_mats:
            sign_mats[colour] = make_material(f'Sign{colour.capitalize()}', base=C[colour])
        curve = bpy.data.curves.new(f'SignText{i}', 'FONT')
        curve.body = text
        curve.size = size
        curve.extrude = 0.05
        curve.align_x = 'CENTER'
        curve.align_y = 'CENTER'
        curve.resolution_u = 2
        tmp = bpy.data.objects.new('tmp', curve)
        coll.objects.link(tmp)
        tmp.rotation_euler = (math.pi / 2, 0, rot)
        tmp.location = (x, y, z)
        depsgraph.update()
        mesh = bpy.data.meshes.new_from_object(tmp.evaluated_get(bpy.context.evaluated_depsgraph_get()))
        mesh.materials.clear()
        mesh.materials.append(sign_mats[colour])
        obj = bpy.data.objects.new(f'NOCOL_Sign_{text.replace(" ", "_").replace("/", "_")}', mesh)
        obj.matrix_world = tmp.matrix_world.copy()
        coll.objects.link(obj)
        bpy.data.objects.remove(tmp)
        objects.append(obj)
    return objects


def setup_render(path, top_down=False, closeup=False):
    scene = bpy.context.scene
    scene.render.engine = 'CYCLES'
    scene.cycles.device = 'CPU'
    scene.cycles.samples = 24
    scene.cycles.use_denoising = True
    scene.render.resolution_x, scene.render.resolution_y = (1400, 1400) if top_down else (1600, 1000)
    scene.view_settings.view_transform = 'Standard'
    world = bpy.data.worlds.new('Sky')
    world.use_nodes = True
    world.node_tree.nodes['Background'].inputs['Color'].default_value = (0.55, 0.72, 0.95, 1)
    world.node_tree.nodes['Background'].inputs['Strength'].default_value = 0.9
    scene.world = world
    if 'Sun' not in bpy.data.objects:
        sun = bpy.data.lights.new('Sun', 'SUN')
        sun.energy = 3.5
        sun.angle = math.radians(8)
        sun_obj = bpy.data.objects.new('Sun', sun)
        sun_obj.rotation_euler = (math.radians(50), math.radians(10), math.radians(35))
        scene.collection.objects.link(sun_obj)
    # Show the colour attribute in the render (export uses it directly).
    for mat in bpy.data.materials:
        if not mat.use_nodes or mat.name.startswith('Sign'):
            continue
        nodes, links = mat.node_tree.nodes, mat.node_tree.links
        bsdf = nodes['Principled BSDF']
        if any(n.type == 'VERTEX_COLOR' for n in nodes):
            continue
        attr = nodes.new('ShaderNodeVertexColor')
        attr.layer_name = 'Color'
        mix = nodes.new('ShaderNodeMix')
        mix.data_type = 'RGBA'
        mix.blend_type = 'MULTIPLY'
        mix.inputs['Factor'].default_value = 1.0
        source = bsdf.inputs['Base Color'].links[0].from_socket if bsdf.inputs['Base Color'].is_linked else None
        # The RGBA sockets of the Mix node are inputs 6/7 and output 2.
        if source is not None:
            links.new(source, mix.inputs[6])
        else:
            mix.inputs[6].default_value = bsdf.inputs['Base Color'].default_value
        # SARE shows colour values as they are; Cycles treats them as linear.
        gamma = nodes.new('ShaderNodeGamma')
        gamma.inputs['Gamma'].default_value = 2.2
        links.new(attr.outputs['Color'], gamma.inputs['Color'])
        links.new(gamma.outputs['Color'], mix.inputs[7])
        links.new(mix.outputs[2], bsdf.inputs['Base Color'])
    cam_data = bpy.data.cameras.new('Camera')
    cam = bpy.data.objects.new('Camera', cam_data)
    scene.collection.objects.link(cam)
    if top_down:
        cam_data.type = 'ORTHO'
        cam_data.ortho_scale = 2 * EDGE + 10
        cam.location = (0, 0, 300)
        cam.rotation_euler = (0, 0, 0)
    elif closeup:
        cam_data.lens = 24
        cam.location = (2, 20, 6)
        cam.rotation_euler = (math.radians(84), 0, math.radians(-28))
    else:
        cam_data.lens = 32
        cam.location = (175, -215, 125)
        cam.rotation_euler = (math.radians(62), 0, math.radians(38))
    scene.camera = cam
    scene.render.filepath = path
    bpy.ops.render.render(write_still=True)
    bpy.data.objects.remove(cam)


def write_locations(path):
    def world(p):
        x, y, z, h = p
        return '{ x = %.2f, y = %.2f, z = %.2f, heading = %.1f }' % (ANCHOR[0] + x, ANCHOR[1] + y, z, h)

    lines = [
        '-- GENERATED by tools/blender/make_rp_town.py from its LOCATIONS table.',
        '-- San Andreas world coordinates of the SARE RP Town content pack',
        '-- (Blender x, y become 2500 + x, -1670 + y). Change the map and these',
        '-- together: edit LOCATIONS in the script and run it again.',
        'RPLocations = {',
    ]
    for key, value in LOCATIONS.items():
        if isinstance(value, list):
            lines.append(f'  {key} = {{')
            for p in value:
                lines.append(f'    {world(p)},')
            lines.append('  },')
        else:
            lines.append(f'  {key} = {world(value)},')
    lines.append('}')
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, 'w', encoding='utf-8', newline='\n') as f:
        f.write('\n'.join(lines) + '\n')
    # Spawn points for spawnmanager, in the map resource next to the core.
    spawns = ['-- GENERATED by tools/blender/make_rp_town.py: new characters start in',
              '-- Legion Square of SARE RP Town (San Andreas world coordinates).']
    for i, p in enumerate(LOCATIONS['spawns']):
        spawns.append("spawnpoint 'rp_spawn%d' %s" % (i + 1, world(p)))
    map_path = os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(path))), 'sare-rp-map', 'map.lua')
    os.makedirs(os.path.dirname(map_path), exist_ok=True)
    with open(map_path, 'w', encoding='utf-8', newline='\n') as f:
        f.write('\n'.join(spawns) + '\n')


def main():
    opts = args()
    bpy.ops.wm.read_factory_settings(use_empty=True)
    build_ground()
    build_blocks()
    build_police()
    build_hospital()
    build_bank_and_shop()
    build_park()
    build_east()
    build_west()
    build_north()
    build_south()
    build_edges()
    textures = make_textures()
    objects = build_objects(textures)
    tris = sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in objects)
    print(f'SARE RP Town: {len(objects)} objects, {tris} triangles')
    if opts['export']:
        os.makedirs(os.path.dirname(opts['out']), exist_ok=True)
        bpy.ops.export_scene.gltf(filepath=opts['out'], export_format='GLB', export_yup=True,
                                  export_apply=True, export_vertex_color='ACTIVE',
                                  export_all_vertex_colors=False, export_normals=True,
                                  export_texcoords=True, export_cameras=False, export_lights=False,
                                  export_extras=False, export_image_format='AUTO')
        print('wrote', opts['out'], os.path.getsize(opts['out']), 'bytes')
        write_locations(opts['lua'])
        print('wrote', opts['lua'])
    if opts['blend']:
        bpy.ops.wm.save_as_mainfile(filepath=opts['blend'])
    if opts['render']:
        setup_render(opts['render'])
    if opts['map']:
        setup_render(opts['map'], top_down=True)
    if opts['closeup']:
        setup_render(opts['closeup'], closeup=True)


if __name__ == '__main__':
    main()
