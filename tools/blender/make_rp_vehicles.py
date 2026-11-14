"""Build the SARE RP Town vehicles in Blender: low-poly cars in the town's
style with separate wheels, one .glb per vehicle.

Run headless from the repository root:

    blender -b --factory-startup --python tools/blender/make_rp_vehicles.py

Options after `--`:
    --dir PATH       folder for the .glb files (default content/sare-rp-town/vehicles)
    --manifest PATH  content.json whose "vehicles" list is rewritten
                     (default content/sare-rp-town/content.json)
    --render PATH    render a line-up of every vehicle (Cycles, CPU)
    --no-export      only build (with --render)

How SARE reads a vehicle (see docs/content-packs.md, "Vehicles"):
- The car's nose points along Blender +Y; X is its width. The origin is the
  car's centre at roughly axle height: the lowest point (the tyres) is
  where it touches the road.
- Objects named `wheel_lf`, `wheel_rf`, `wheel_lb`, `wheel_rb` are the wheels
  (left is -X). Their origin is the hub; front wheels steer, all of them roll.
- The material `paint` takes the car's colour (the dealer's colour choice);
  other materials keep their own colour.
- The collision box comes from the model's size.

Everything is generated from code: CC0, like the script.
"""
import bpy
import bmesh
import json
import math
import os
import sys

from mathutils import Matrix, Vector

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))

COLORS = {
    'paint': (1.0, 1.0, 1.0),
    'glass': (0.10, 0.13, 0.17),
    'trim': (0.07, 0.07, 0.08),
    'arch': (0.03, 0.03, 0.035),
    'chrome': (0.72, 0.73, 0.75),
    'tire': (0.05, 0.05, 0.05),
    'rim': (0.62, 0.63, 0.66),
    'headlight': (1.0, 0.96, 0.78),
    'taillight': (0.75, 0.06, 0.05),
    'indicator': (1.0, 0.55, 0.08),
    'interior': (0.16, 0.15, 0.15),
    'white': (0.94, 0.94, 0.94),
    'blue': (0.05, 0.2, 0.95),
    'red': (0.9, 0.05, 0.05),
    'amber': (1.0, 0.6, 0.05),
    'yellow': (1.0, 0.8, 0.1),
    'black': (0.04, 0.04, 0.05),
}


def args():
    argv = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []
    opts = {'dir': os.path.join(ROOT, 'content/sare-rp-town/vehicles'),
            'manifest': os.path.join(ROOT, 'content/sare-rp-town/content.json'),
            'render': None, 'export': True}
    i = 0
    while i < len(argv):
        if argv[i] == '--no-export':
            opts['export'] = False
            i += 1
            continue
        opts[argv[i].lstrip('-')] = os.path.abspath(argv[i + 1])
        i += 2
    return opts


def srgb(rgb):
    def lin(c):
        c /= 255.0
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4
    return tuple(lin(float(c)) for c in rgb)


def material(name, rgb):
    mat = bpy.data.materials.get(name)
    if mat:
        return mat
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes['Principled BSDF']
    bsdf.inputs['Base Color'].default_value = (*rgb, 1.0)
    bsdf.inputs['Roughness'].default_value = 0.35 if name in ('paint', 'glass', 'chrome') else 0.8
    if name in ('headlight', 'taillight', 'blue', 'red', 'amber'):
        bsdf.inputs['Emission Color'].default_value = (*rgb, 1.0)
        bsdf.inputs['Emission Strength'].default_value = 0.6
    alpha = 0.5 if name == 'glass' else 1.0
    if alpha < 1.0:
        # Tinted see-through glass, so the seated driver shows (glTF BLEND).
        bsdf.inputs['Alpha'].default_value = alpha
        mat.surface_render_method = 'BLENDED'
    mat.diffuse_color = (*rgb, alpha)
    return mat


class Car:
    def __init__(self, ident, paint_material):
        self.ident = ident
        self.objects = []
        self.paint = paint_material

    def mat(self, name):
        if name == 'paint':
            return self.paint
        return material(name, COLORS[name])

    def add(self, name, bm, mat, location=(0, 0, 0)):
        mesh = bpy.data.meshes.new(f'{self.ident}_{name}')
        bm.normal_update()
        bm.to_mesh(mesh)
        bm.free()
        mesh.materials.append(self.mat(mat))
        obj = bpy.data.objects.new(name, mesh)
        obj.location = location
        bpy.context.scene.collection.objects.link(obj)
        self.objects.append(obj)
        return obj

    def box(self, name, center, size, mat):
        bm = bmesh.new()
        bmesh.ops.create_cube(bm, size=1.0)
        bmesh.ops.scale(bm, vec=size, verts=bm.verts)
        bmesh.ops.translate(bm, vec=center, verts=bm.verts)
        return self.add(name, bm, mat)

    def profile(self, name, points, width, mat, taper=1.0, taper_from=None, x0=None):
        """Extrude a side profile [(y, z), ...] across the car's width.
        Points above `taper_from` move in towards the middle by `taper`."""
        bm = bmesh.new()
        half = width / 2
        left = [bm.verts.new((-half if x0 is None else x0, y, z)) for y, z in points]
        face = bm.faces.new(left)
        ext = bmesh.ops.extrude_face_region(bm, geom=[face])
        moved = [g for g in ext['geom'] if isinstance(g, bmesh.types.BMVert)]
        bmesh.ops.translate(bm, vec=(width, 0, 0), verts=moved)
        if taper != 1.0 and taper_from is not None:
            for v in bm.verts:
                if v.co.z > taper_from:
                    v.co.x *= taper
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        return self.add(name, bm, mat)

    def wheel(self, name, hub, radius, width):
        bm = bmesh.new()
        bmesh.ops.create_cone(bm, cap_ends=True, segments=14, radius1=radius, radius2=radius, depth=width)
        bmesh.ops.rotate(bm, verts=bm.verts, matrix=Matrix.Rotation(math.radians(90), 3, 'Y'))
        tire = self.add(name, bm, 'tire', hub)
        # The rim is part of the wheel object so it rolls with it.
        rim = bmesh.new()
        bmesh.ops.create_cone(rim, cap_ends=True, segments=10, radius1=radius * 0.62, radius2=radius * 0.62, depth=width + 0.02)
        bmesh.ops.rotate(rim, verts=rim.verts, matrix=Matrix.Rotation(math.radians(90), 3, 'Y'))
        hubcap = bmesh.new()
        bmesh.ops.create_cube(hubcap, size=1.0)
        bmesh.ops.scale(hubcap, vec=(width + 0.04, radius * 0.25, radius * 0.25), verts=hubcap.verts)
        rim_obj = self.add(f'{name}_rim', rim, 'rim', hub)
        cap_obj = self.add(f'{name}_cap', hubcap, 'chrome', hub)
        # Join rim and cap into the tyre so the wheel is one object.
        for extra in (rim_obj, cap_obj):
            extra.data.materials.clear()
        join(tire, [rim_obj, cap_obj], self, ['rim', 'chrome'])
        return tire

    def text(self, name, body, center, size, mat, side):
        curve = bpy.data.curves.new(f'{self.ident}_{name}', 'FONT')
        curve.body = body
        curve.size = size
        curve.align_x = 'CENTER'
        curve.align_y = 'CENTER'
        curve.extrude = 0.004
        obj = bpy.data.objects.new(name, curve)
        bpy.context.scene.collection.objects.link(obj)
        obj.location = center
        # Side text reads from outside the car; 'L' is the -X side.
        obj.rotation_euler = (math.radians(90), 0, math.radians(-90 if side == 'L' else 90))
        bpy.context.view_layer.objects.active = obj
        for o in bpy.context.selected_objects:
            o.select_set(False)
        obj.select_set(True)
        bpy.ops.object.convert(target='MESH')
        obj = bpy.context.view_layer.objects.active
        obj.data.materials.clear()
        obj.data.materials.append(self.mat(mat))
        self.objects.append(obj)
        return obj


def join(target, others, car, mats):
    for m in mats:
        target.data.materials.append(car.mat(m))
    offset = {o.name: i + 1 for i, o in enumerate(others)}
    for o in others:
        for poly in o.data.polygons:
            poly.material_index = offset[o.name]
    for o in bpy.context.selected_objects:
        o.select_set(False)
    for o in others:
        o.select_set(True)
        car.objects.remove(o)
    target.select_set(True)
    bpy.context.view_layer.objects.active = target
    bpy.ops.object.join()


# --- Shared parts -------------------------------------------------------------------
def wheels_and_arches(car, spec):
    r, w, half = spec['wheel'], 0.24, spec['width'] / 2
    front, rear = spec['axles']
    for tag, y in (('f', front), ('b', rear)):
        for side, sx in (('l', -1), ('r', 1)):
            car.wheel(f'wheel_{side}{tag}', (sx * (half - w / 2 + 0.02), y, r), r, w)
            car.box(f'arch_{side}{tag}', (sx * (half + 0.004), y, r + 0.05), (0.012, 2 * r + 0.16, 2 * r * 0.8), 'arch')


def lights(car, spec, z=0.66, rear_z=None):
    L, half = spec['length'] / 2, spec['width'] / 2
    rear_z = rear_z or z + 0.06
    for sx in (-1, 1):
        car.box(f'headlight{sx}', (sx * (half - 0.25), L + 0.005, z), (0.32, 0.03, 0.12), 'headlight')
        car.box(f'indicator{sx}', (sx * (half - 0.07), L - 0.01, z), (0.08, 0.03, 0.1), 'indicator')
        car.box(f'taillight{sx}', (sx * (half - 0.22), -L - 0.005, rear_z), (0.3, 0.03, 0.12), 'taillight')


def bumpers(car, spec, z=0.38, h=0.18):
    L, w = spec['length'] / 2, spec['width']
    car.box('bumper_front', (0, L + 0.02, z), (w * 0.98, 0.12, h), 'trim')
    car.box('bumper_rear', (0, -L - 0.02, z), (w * 0.98, 0.12, h), 'trim')
    car.box('grille', (0, L + 0.008, z + h / 2 + 0.1), (w * 0.4, 0.02, 0.12), 'trim')
    car.box('plate_front', (0, L + 0.085, z), (0.42, 0.01, 0.1), 'white')
    car.box('plate_rear', (0, -L - 0.085, z + 0.02), (0.42, 0.01, 0.1), 'white')


def mirrors(car, spec, y, z):
    half = spec['width'] / 2
    for sx in (-1, 1):
        car.box(f'mirror{sx}', (sx * (half + 0.07), y, z), (0.12, 0.06, 0.08), 'paint')


def roof_and_pillars(car, cabin, width, taper):
    """A painted roof on the glass cabin and pillars at its corners."""
    (fy, fz), (ay, az), (by, bz), (ry, rz) = cabin
    rw = width * taper
    car.box('roof', (0, (ay + by) / 2, max(az, bz) + 0.02), (rw + 0.02, abs(ay - by) + 0.06, 0.05), 'paint')
    for sx in (-1, 1):
        x = sx * (rw / 2 + 0.006)
        mid = (ay + by) / 2
        car.box(f'pillar_b{sx}', (x, mid, (fz + az) / 2 + 0.02), (0.02, 0.1, abs(az - fz) - 0.05), 'paint')


# --- Vehicles ---------------------------------------------------------------------------
def saloon(car, spec, cabin_top=1.42):
    """Sedan, police cruiser, taxi."""
    L = spec['length'] / 2
    lower = [(-L + 0.02, 0.3), (L - 0.02, 0.3), (L + 0.02, 0.55), (L - 0.05, 0.8), (L - 1.35, 0.9),
             (-L + 0.75, 0.93), (-L + 0.05, 0.88), (-L - 0.02, 0.6)]
    car.profile('body', lower, spec['width'], 'paint')
    cabin = [(L - 1.38, 0.88), (L - 2.15, cabin_top - 0.02), (-L + 1.35, cabin_top), (-L + 0.72, 0.9)]
    car.profile('cabin', cabin, spec['width'] * 0.9, 'glass', taper=0.84, taper_from=1.0)
    roof_and_pillars(car, cabin, spec['width'] * 0.9, 0.84)
    car.box('interior', (0, -0.2, 0.86), (spec['width'] * 0.8, 1.9, 0.06), 'interior')
    mirrors(car, spec, L - 1.45, 1.0)
    wheels_and_arches(car, spec)
    lights(car, spec)
    bumpers(car, spec)


def sedan(car, spec):
    saloon(car, spec)


def police(car, spec):
    saloon(car, spec)
    half, L = spec['width'] / 2, spec['length'] / 2
    for sx, side in ((-1, 'L'), (1, 'R')):
        car.box(f'door_panel{sx}', (sx * (half + 0.006), -0.15, 0.62), (0.012, 2.1, 0.5), 'white')
        car.text(f'police_text{sx}', 'POLICE', (sx * (half + 0.016), -0.15, 0.62), 0.24, 'black', side)
    car.box('lightbar_base', (0, -0.15, 1.47), (1.15, 0.26, 0.05), 'black')
    car.box('lightbar_red', (-0.3, -0.15, 1.53), (0.5, 0.22, 0.08), 'red')
    car.box('lightbar_blue', (0.3, -0.15, 1.53), (0.5, 0.22, 0.08), 'blue')
    car.box('push_bar', (0, L + 0.13, 0.55), (0.9, 0.06, 0.38), 'black')


def taxi(car, spec):
    saloon(car, spec)
    half = spec['width'] / 2
    car.box('sign', (0, -0.15, 1.53), (0.7, 0.22, 0.16), 'yellow')
    for sx, side in ((-1, 'L'), (1, 'R')):
        car.box(f'checker{sx}', (sx * (half + 0.006), -0.1, 0.72), (0.012, 3.0, 0.08), 'black')
    car.text('taxi_text_f', 'TAXI', (0, -0.15 + 0.115, 1.53), 0.12, 'black', 'F')
    car.objects[-1].rotation_euler = (math.radians(90), 0, math.radians(180))


def compact(car, spec):
    L = spec['length'] / 2
    lower = [(-L, 0.3), (L - 0.02, 0.3), (L + 0.03, 0.55), (L - 0.05, 0.78), (L - 0.85, 0.87),
             (-L + 0.05, 0.9), (-L - 0.02, 0.6)]
    car.profile('body', lower, spec['width'], 'paint')
    cabin = [(L - 0.88, 0.85), (L - 1.65, 1.42), (-L + 0.3, 1.44), (-L + 0.02, 0.9)]
    car.profile('cabin', cabin, spec['width'] * 0.9, 'glass', taper=0.86, taper_from=1.0)
    roof_and_pillars(car, cabin, spec['width'] * 0.9, 0.86)
    mirrors(car, spec, L - 0.95, 0.98)
    wheels_and_arches(car, spec)
    lights(car, spec, 0.66, 0.95)
    bumpers(car, spec)


def suv(car, spec):
    L = spec['length'] / 2
    lower = [(-L + 0.02, 0.42), (L - 0.02, 0.42), (L + 0.03, 0.72), (L - 0.04, 1.05), (L - 1.15, 1.12),
             (-L + 0.03, 1.13), (-L - 0.02, 0.75)]
    car.profile('body', lower, spec['width'], 'paint')
    cabin = [(L - 1.18, 1.1), (L - 1.8, 1.78), (-L + 0.12, 1.8), (-L + 0.02, 1.12)]
    car.profile('cabin', cabin, spec['width'] * 0.92, 'glass', taper=0.9, taper_from=1.2)
    roof_and_pillars(car, cabin, spec['width'] * 0.92, 0.9)
    for sx in (-1, 1):
        car.box(f'rail{sx}', (sx * 0.7, -0.2, 1.86), (0.05, 2.6, 0.05), 'trim')
        car.box(f'step{sx}', (sx * (spec['width'] / 2 + 0.05), 0.05, 0.38), (0.1, 2.0, 0.05), 'trim')
    mirrors(car, spec, L - 1.25, 1.25)
    wheels_and_arches(car, spec)
    lights(car, spec, 0.9, 1.0)
    bumpers(car, spec, 0.5, 0.22)


def pickup_cab(car, spec, rear_end):
    """Front body and cab of the pickup and the tow truck; returns the cab's back."""
    L = spec['length'] / 2
    lower = [(rear_end, 0.45), (L - 0.02, 0.45), (L + 0.03, 0.72), (L - 0.03, 1.05), (L - 1.25, 1.1),
             (rear_end, 1.1)]
    car.profile('body', lower, spec['width'], 'paint')
    cab_back = L - 2.75
    cabin = [(L - 1.28, 1.08), (L - 1.85, 1.8), (cab_back + 0.05, 1.82), (cab_back, 1.1)]
    car.profile('cabin', cabin, spec['width'] * 0.92, 'glass', taper=0.9, taper_from=1.2)
    roof_and_pillars(car, cabin, spec['width'] * 0.92, 0.9)
    car.box('cab_back', (0, cab_back - 0.03, 1.45), (spec['width'] * 0.83, 0.06, 0.7), 'paint')
    mirrors(car, spec, L - 1.35, 1.3)
    return cab_back


def pickup(car, spec):
    L, half = spec['length'] / 2, spec['width'] / 2
    cab_back = pickup_cab(car, spec, -L)
    bed = cab_back - 0.08
    for sx in (-1, 1):
        car.box(f'bed_side{sx}', (sx * (half - 0.04), (bed - L) / 2, 1.27), (0.08, bed + L, 0.34), 'paint')
    car.box('tailgate', (0, -L + 0.04, 1.27), (spec['width'], 0.08, 0.34), 'paint')
    car.box('bed_floor', (0, (bed - L) / 2, 1.12), (spec['width'] - 0.1, bed + L, 0.03), 'trim')
    wheels_and_arches(car, spec)
    lights(car, spec, 0.92, 1.25)
    bumpers(car, spec, 0.52, 0.2)


def towtruck(car, spec):
    L, half = spec['length'] / 2, spec['width'] / 2
    cab_back = pickup_cab(car, spec, -L)
    car.box('bed', (0, (cab_back - L) / 2 - 0.05, 1.18), (spec['width'], cab_back + L - 0.1, 0.14), 'trim')
    car.box('boom_base', (0, -L + 0.8, 1.45), (0.4, 0.5, 0.4), 'yellow')
    boom = bmesh.new()
    bmesh.ops.create_cube(boom, size=1.0)
    bmesh.ops.scale(boom, vec=(0.16, 2.0, 0.16), verts=boom.verts)
    bmesh.ops.rotate(boom, verts=boom.verts, matrix=Matrix.Rotation(math.radians(-28), 3, 'X'))
    bmesh.ops.translate(boom, vec=(0, -L + 0.2, 1.95), verts=boom.verts)
    car.add('boom', boom, 'yellow')
    car.box('hook_cable', (0, -L - 0.62, 1.95), (0.03, 0.03, 0.9), 'black')
    car.box('hook', (0, -L - 0.62, 1.48), (0.12, 0.05, 0.1), 'chrome')
    car.box('lightbar', (0, cab_back + 0.85, 1.88), (1.0, 0.22, 0.08), 'amber')
    for sx, side in ((-1, 'L'), (1, 'R')):
        car.text(f'tow_text{sx}', 'TOW', (sx * (half + 0.012), 1.25, 0.78), 0.28, 'black', side)
    wheels_and_arches(car, spec)
    lights(car, spec, 0.92, 1.0)
    bumpers(car, spec, 0.52, 0.2)


def ambulance(car, spec):
    L, half = spec['length'] / 2, spec['width'] / 2
    front = [(L - 1.9, 0.45), (L - 0.02, 0.45), (L + 0.03, 0.75), (L - 0.05, 1.1), (L - 0.9, 1.18), (L - 1.9, 1.2)]
    car.profile('body', front, spec['width'] * 0.96, 'paint')
    cabin = [(L - 0.92, 1.16), (L - 1.4, 2.0), (L - 1.95, 2.02), (L - 1.95, 1.18)]
    car.profile('cabin', cabin, spec['width'] * 0.92, 'glass', taper=0.95, taper_from=1.3)
    car.box('cab_roof', (0, L - 1.65, 2.05), (spec['width'] * 0.9, 0.62, 0.06), 'paint')
    box_front = L - 1.95
    car.box('box', (0, (box_front - L) / 2, 1.58), (spec['width'], box_front + L, 1.98), 'paint')
    car.box('box_floor', (0, (box_front - L) / 2, 0.55), (spec['width'] * 0.96, box_front + L, 0.22), 'trim')
    for sx, side in ((-1, 'L'), (1, 'R')):
        car.box(f'stripe{sx}', (sx * (half + 0.006), (box_front - L) / 2, 1.2), (0.012, box_front + L, 0.22), 'red')
        car.text(f'amb_text{sx}', 'AMBULANCE', (sx * (half + 0.014), (box_front - L) / 2, 1.62), 0.3, 'red', side)
        car.box(f'window{sx}', (sx * (half + 0.006), -0.4, 2.05), (0.012, 0.8, 0.4), 'glass')
    car.box('cross_bar', (0, -L - 0.008, 1.95), (0.5, 0.012, 0.14), 'red')
    car.box('cross_up', (0, -L - 0.008, 1.95), (0.14, 0.012, 0.5), 'red')
    car.box('rear_seam', (0, -L - 0.006, 1.3), (0.02, 0.012, 1.4), 'trim')
    car.box('light_r', (-0.6, box_front - 0.15, 2.62), (0.3, 0.15, 0.1), 'red')
    car.box('light_b', (0.6, box_front - 0.15, 2.62), (0.3, 0.15, 0.1), 'blue')
    car.box('light_rr', (-0.8, -L + 0.1, 2.62), (0.2, 0.12, 0.1), 'red')
    car.box('light_rb', (0.8, -L + 0.1, 2.62), (0.2, 0.12, 0.1), 'blue')
    mirrors(car, spec, L - 1.0, 1.45)
    wheels_and_arches(car, spec)
    lights(car, spec, 0.92, 0.9)
    bumpers(car, spec, 0.5, 0.22)


def sports(car, spec):
    L = spec['length'] / 2
    lower = [(-L + 0.03, 0.26), (L - 0.05, 0.26), (L + 0.02, 0.4), (L - 0.12, 0.58), (L - 1.45, 0.76),
             (-L + 0.55, 0.8), (-L + 0.02, 0.76), (-L - 0.02, 0.48)]
    car.profile('body', lower, spec['width'], 'paint')
    cabin = [(L - 1.5, 0.74), (L - 2.3, 1.22), (-L + 1.45, 1.24), (-L + 0.62, 0.79)]
    car.profile('cabin', cabin, spec['width'] * 0.86, 'glass', taper=0.82, taper_from=0.85)
    roof_and_pillars(car, cabin, spec['width'] * 0.86, 0.82)
    car.box('spoiler', (0, -L + 0.2, 0.98), (spec['width'] * 0.9, 0.28, 0.04), 'trim')
    for sx in (-1, 1):
        car.box(f'spoiler_leg{sx}', (sx * 0.6, -L + 0.2, 0.88), (0.05, 0.12, 0.18), 'trim')
        car.box(f'intake{sx}', (sx * (spec['width'] / 2 + 0.005), -0.3, 0.55), (0.012, 0.6, 0.14), 'trim')
    mirrors(car, spec, L - 1.6, 0.86)
    wheels_and_arches(car, spec)
    lights(car, spec, 0.48, 0.62)
    bumpers(car, spec, 0.3, 0.12)


# id, name, builder, spec, default colour (sRGB 0-255), handling overrides
VEHICLES = [
    ('sedan', 'Sedan', sedan, dict(length=4.6, width=1.82, wheel=0.33, axles=(1.38, -1.32), origin=0.55),
     (60, 90, 150), {'top_speed': 46.0, 'acceleration': 6.0}),
    ('compact', 'Compact', compact, dict(length=3.8, width=1.7, wheel=0.3, axles=(1.2, -1.18), origin=0.5),
     (200, 60, 50), {'top_speed': 40.0, 'acceleration': 5.4, 'tire_grip': 4.8}),
    ('suv', 'SUV', suv, dict(length=4.8, width=1.96, wheel=0.38, axles=(1.45, -1.42), origin=0.66),
     (40, 44, 48), {'top_speed': 44.0, 'acceleration': 5.8, 'cg_offset': 0.15}),
    ('pickup', 'Pickup', pickup, dict(length=5.3, width=1.96, wheel=0.38, axles=(1.7, -1.55), origin=0.66),
     (170, 160, 140), {'top_speed': 43.0, 'acceleration': 5.6, 'drive': 1.0}),
    ('sports', 'Sports car', sports, dict(length=4.4, width=1.92, wheel=0.34, axles=(1.35, -1.3), origin=0.5),
     (240, 200, 30), {'top_speed': 62.0, 'acceleration': 9.0, 'tire_grip': 5.6, 'drive': 1.0}),
    ('police', 'Police cruiser', police, dict(length=4.7, width=1.84, wheel=0.34, axles=(1.42, -1.35), origin=0.56),
     (25, 25, 30), {'top_speed': 54.0, 'acceleration': 7.4, 'tire_grip': 5.0}),
    ('taxi', 'Taxi', taxi, dict(length=4.6, width=1.82, wheel=0.33, axles=(1.38, -1.32), origin=0.55),
     (250, 200, 30), {'top_speed': 46.0, 'acceleration': 6.0}),
    ('ambulance', 'Ambulance', ambulance, dict(length=5.9, width=2.1, wheel=0.4, axles=(1.95, -1.6), origin=0.7),
     (240, 240, 240), {'top_speed': 42.0, 'acceleration': 5.2, 'cg_offset': 0.3}),
    ('towtruck', 'Tow truck', towtruck, dict(length=5.8, width=2.0, wheel=0.4, axles=(1.9, -1.7), origin=0.68),
     (230, 120, 30), {'top_speed': 40.0, 'acceleration': 5.0, 'drive': 1.0}),
]


# Driver seat: (forward of the origin, height of the seated character's feet
# above the road). The sit clip puts the hips 0.5 m above the feet.
SEATS = {
    'sedan': (-0.05, 0.02), 'police': (-0.05, 0.02), 'taxi': (-0.05, 0.02),
    'compact': (-0.1, 0.05), 'suv': (0.0, 0.35), 'pickup': (0.35, 0.4),
    'sports': (-0.35, -0.06), 'ambulance': (1.35, 0.5), 'towtruck': (0.6, 0.4),
}


def seat(ident, spec):
    forward, feet = SEATS[ident]
    return [round(-spec['width'] * 0.22, 3), forward, round(feet - spec['origin'], 3)]


def clear():
    for obj in list(bpy.data.objects):
        bpy.data.objects.remove(obj, do_unlink=True)


def build(entry, paint_rgb=None, location=None, heading=0.0, keep=False):
    ident, name, fn, spec, color, _ = entry
    paint = material('paint', COLORS['paint']) if paint_rgb is None else material(f'paint_{ident}_{location}', srgb(paint_rgb))
    car = Car(ident, paint)
    fn(car, spec)
    # Origin at axle height: SARE puts the lowest point on the road.
    for obj in car.objects:
        obj.location.z -= spec['origin']
    if location is not None:
        root = bpy.data.objects.new(f'{ident}_root', None)
        bpy.context.scene.collection.objects.link(root)
        for obj in car.objects:
            obj.parent = root
        root.location = location
        root.rotation_euler = (0, 0, heading)
        root.location.z += spec['origin']
    return car


def export(entry, folder):
    clear()
    build(entry)
    path = os.path.join(folder, f'{entry[0]}.glb')
    bpy.ops.export_scene.gltf(filepath=path, export_format='GLB', export_yup=True, export_apply=True,
                              export_normals=True, export_texcoords=False, export_vertex_color='NONE',
                              export_cameras=False, export_lights=False, export_extras=False,
                              export_animations=False, export_materials='EXPORT')
    print('wrote', path, os.path.getsize(path), 'bytes')


def update_manifest(path):
    with open(path, encoding='utf-8') as f:
        manifest = json.load(f)
    manifest['vehicles'] = [
        {'model': ident, 'name': name, 'scene': f'vehicles/{ident}.glb', 'color': list(color),
         'seat': seat(ident, spec), 'handling': handling}
        for ident, name, _, spec, color, handling in VEHICLES
    ]
    with open(path, 'w', encoding='utf-8') as f:
        json.dump(manifest, f, indent=2)
        f.write('\n')
    print('updated', path)


def render_lineup(path):
    clear()
    scene = bpy.context.scene
    for i, entry in enumerate(VEHICLES):
        row, col = divmod(i, 5)
        x = (col - 2) * 3.6 + (1.8 if row else 0)
        y = row * 7.5
        build(entry, entry[4], (x, y, 0), math.radians(215))
    g = bpy.data.meshes.new('ground')
    bm = bmesh.new()
    bmesh.ops.create_grid(bm, x_segments=1, y_segments=1, size=40)
    bm.to_mesh(g)
    bm.free()
    ground = bpy.data.objects.new('ground', g)
    ground.data.materials.append(material('ground', (0.2, 0.21, 0.23)))
    scene.collection.objects.link(ground)
    cam = bpy.data.objects.new('cam', bpy.data.cameras.new('cam'))
    cam.data.lens = 35
    scene.collection.objects.link(cam)
    cam.location = (0.0, -16.5, 10.0)
    cam.rotation_euler = (math.radians(62), 0, 0)
    scene.camera = cam
    sun = bpy.data.objects.new('sun', bpy.data.lights.new('sun', 'SUN'))
    sun.data.energy = 3.5
    sun.data.angle = math.radians(6)
    sun.rotation_euler = (math.radians(45), math.radians(12), math.radians(-30))
    scene.collection.objects.link(sun)
    scene.render.engine = 'CYCLES'
    scene.cycles.device = 'CPU'
    scene.cycles.samples = 32
    scene.cycles.use_denoising = True
    scene.render.resolution_x, scene.render.resolution_y = 1800, 1000
    scene.render.filepath = path
    scene.world = scene.world or bpy.data.worlds.new('World')
    scene.world.use_nodes = True
    scene.world.node_tree.nodes['Background'].inputs['Color'].default_value = (0.62, 0.75, 0.95, 1)
    scene.view_settings.view_transform = 'Standard'
    bpy.ops.render.render(write_still=True)
    print('rendered', path)


def main():
    opts = args()
    if opts['export']:
        os.makedirs(opts['dir'], exist_ok=True)
        for entry in VEHICLES:
            export(entry, opts['dir'])
        update_manifest(opts['manifest'])
    if opts['render']:
        render_lineup(opts['render'])


if __name__ == '__main__':
    main()
