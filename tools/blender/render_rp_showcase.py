"""Render a street scene of SARE RP Town with the pack's cars and characters.

    blender -b --factory-startup --python tools/blender/render_rp_showcase.py -- --out shot.png
        [--camera x,y,z] [--target x,y,z]

It rebuilds the town with make_rp_town.py and places cars from
make_rp_vehicles.py and dressed characters from make_rp_characters.py, so the
picture shows the same models the content pack ships. Nothing is exported.
Everything is generated from code: CC0, like the other scripts.
"""
import math
import os
import sys

import bpy
from mathutils import Matrix, Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import make_rp_town as T  # noqa: E402
import make_rp_vehicles as V  # noqa: E402
import make_rp_characters as C  # noqa: E402


def out_path():
    argv = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []
    if '--out' in argv:
        return os.path.abspath(argv[argv.index('--out') + 1])
    return os.path.join(T.ROOT if hasattr(T, 'ROOT') else os.getcwd(), 'rp_town_showcase.png')


def vec_arg(name, default):
    argv = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []
    if name in argv:
        return tuple(float(v) for v in argv[argv.index(name) + 1].split(','))
    return default


def town_colours():
    """The town stores colours per vertex; show them in Cycles like make_rp_town's renders."""
    for mat in bpy.data.materials:
        if not mat.use_nodes or mat.name.startswith('Sign'):
            continue
        nodes, links = mat.node_tree.nodes, mat.node_tree.links
        bsdf = nodes['Principled BSDF']
        attr = nodes.new('ShaderNodeVertexColor')
        attr.layer_name = 'Color'
        mix = nodes.new('ShaderNodeMix')
        mix.data_type = 'RGBA'
        mix.blend_type = 'MULTIPLY'
        mix.inputs['Factor'].default_value = 1.0
        source = bsdf.inputs['Base Color'].links[0].from_socket if bsdf.inputs['Base Color'].is_linked else None
        if source is not None:
            links.new(source, mix.inputs[6])
        else:
            mix.inputs[6].default_value = bsdf.inputs['Base Color'].default_value
        gamma = nodes.new('ShaderNodeGamma')
        gamma.inputs['Gamma'].default_value = 2.2
        links.new(attr.outputs['Color'], gamma.inputs['Color'])
        links.new(gamma.outputs['Color'], mix.inputs[7])
        links.new(mix.outputs[2], bsdf.inputs['Base Color'])


def vehicle(ident, x, y, heading_deg, rgb=None):
    entry = next(e for e in V.VEHICLES if e[0] == ident)
    V.build(entry, rgb or entry[4], (x, y, T.ROAD_TOP), math.radians(heading_deg))
    return entry


def driver(entry, x, y, heading_deg, look, kind='M'):
    """A character in the driver's seat, placed the way the engine does it."""
    ident, _, _, spec, _, _ = entry
    right, forward, up = V.seat(ident, spec)
    h = math.radians(heading_deg)
    offset = Matrix.Rotation(h, 3, 'Z') @ Vector((right, forward, up))
    base = Vector((x, y, T.ROAD_TOP + spec['origin'])) + offset
    C.make_look(kind, look, f'driver_{ident}', location=tuple(base), heading=h + math.pi, clip='sit', t=0.0)


def look(skin, hair, hair_color, top, top_color, pants, pants_color, shoes, shoes_color):
    return dict(hair=hair, top=top, pants=pants, shoes=shoes, skin_color=skin, hair_color=hair_color,
                top_color=top_color, pants_color=pants_color, shoes_color=shoes_color)


def main():
    path = out_path()
    bpy.ops.wm.read_factory_settings(use_empty=True)
    for build in (T.build_ground, T.build_blocks, T.build_police, T.build_hospital, T.build_bank_and_shop,
                  T.build_park, T.build_east, T.build_west, T.build_north, T.build_south, T.build_edges):
        build()
    T.build_objects(T.make_textures())
    town_colours()

    # Eastbound lane is y < 0 (cars drive on the right); nose +Y at heading 0.
    police = vehicle('police', -30.0, -1.8, -90)
    driver(police, -30.0, -1.8, -90, look((170, 112, 78), 2, (30, 24, 20), 5, (30, 40, 70), 0, (30, 40, 70), 2, (20, 20, 20)))
    vehicle('taxi', -52.0, 2.8, 90)
    vehicle('sedan', -42.0, -1.8, -90, (40, 90, 170))
    vehicle('sports', -37.0, 2.8, 90, (200, 30, 30))
    vehicle('pickup', -60.0, -1.8, -90, (230, 230, 230))
    vehicle('ambulance', -22.0, 2.8, 90)
    # Parked along the south kerb.
    vehicle('compact', -23.5, -4.6, -90, (250, 200, 40))
    vehicle('suv', -48.0, -4.6, -90, (40, 40, 46))

    walkers = [
        ('F', look((198, 134, 96), 4, (30, 22, 18), 1, (240, 200, 60), 3, (40, 40, 50), 2, (120, 30, 40)), (-35.0, -8.4), 80, 'walk', 0.25),
        ('M', look((224, 172, 140), 1, (60, 40, 28), 0, (200, 40, 40), 0, (50, 70, 120), 0, (240, 240, 240)), (-36.0, -9.2), 60, 'idle', 0.3),
        ('F', look((90, 58, 40), 6, (30, 20, 16), 3, (60, 170, 200), 1, (220, 220, 210), 0, (250, 120, 160)), (-29.0, -9.0), -100, 'walk', 0.7),
        ('M', look((120, 78, 52), 5, (20, 16, 14), 2, (70, 140, 70), 2, (110, 100, 70), 1, (90, 60, 40)), (-40.5, -8.7), -80, 'run', 0.3),
        ('F', look((240, 200, 170), 3, (200, 150, 70), 4, (40, 40, 46), 0, (70, 90, 140), 1, (60, 40, 30)), (-26.0, -8.3), 20, 'idle', 0.6),
    ]
    for i, (kind, lk, (x, y), heading, clip, t) in enumerate(walkers):
        C.make_look(kind, lk, f'walker{i}', location=(x, y, T.BLOCK_TOP), heading=math.radians(heading), clip=clip, t=t)

    scene = bpy.context.scene
    target = bpy.data.objects.new('target', None)
    target.location = vec_arg('--target', (-33.0, -2.0, 1.0))
    scene.collection.objects.link(target)
    cam = bpy.data.objects.new('cam', bpy.data.cameras.new('cam'))
    cam.data.lens = 30
    cam.location = vec_arg('--camera', (-21.0, -15.5, 3.6))
    track = cam.constraints.new('TRACK_TO')
    track.target = target
    track.track_axis = 'TRACK_NEGATIVE_Z'
    track.up_axis = 'UP_Y'
    scene.collection.objects.link(cam)
    scene.camera = cam
    sun = bpy.data.objects.new('sun', bpy.data.lights.new('sun', 'SUN'))
    sun.data.energy = 3.5
    sun.data.angle = math.radians(8)
    sun.rotation_euler = (math.radians(50), math.radians(10), math.radians(35))
    scene.collection.objects.link(sun)
    C.setup_cycles(scene, path, 1800, 1000, samples=32)
    bpy.ops.render.render(write_still=True)
    print('rendered', path)


if __name__ == '__main__':
    main()
