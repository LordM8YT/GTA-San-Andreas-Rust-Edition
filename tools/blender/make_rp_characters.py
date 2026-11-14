"""Build the SARE RP Town characters in Blender: two low-poly bodies (male and
female), rigged with an armature each, with hair styles, tops, trousers and
shoes as separate pieces, and idle/walk/run/sit/fall animations.

Run headless from the repository root:

    blender -b --factory-startup --python tools/blender/make_rp_characters.py

Options after `--`:
    --out PATH       the .glb to write (default content/sare-rp-town/characters.glb)
    --blend PATH     also save the editable .blend file
    --render PATH    render a line-up of characters with different looks (Cycles, CPU)
    --no-export      only build the scene

How SARE reads the file (see docs/content-packs.md, "Characters"):
- Every body is an armature. Its bones become glTF nodes; mesh pieces are
  parented to bones, so they move rigidly with them (no skin weights).
- A piece's object name is `<body>.<slot>.<style>.<part>`: body `M` or `F`,
  slot `base` (always drawn), `hair`, `top`, `pants` or `shoes`, and the
  style number. SARE draws the base and the chosen style of each slot for the
  chosen body, and folds the others away.
- A piece's material name picks its colour: `skin`, `hair`, `top`, `pants`
  and `shoes` take the player's colours; any other material keeps its own.
- Animations (Blender actions) are `idle`, `walk`, `run`, `sit` and `fall`.
  `walk` covers 1.6 m per loop and `run` 2.64 m, so SARE plays them at the
  speed the character moves.

Blender's -Y is the character's front; the feet stand on Z = 0. Everything
is generated from code: CC0, like the script.
"""
import bpy
import bmesh
import math
import os
import sys

from mathutils import Euler, Matrix, Quaternion, Vector

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), '..', '..'))
FPS = 30

# Style names, in the order of their numbers. The server's character
# creator (sare_rp_identity) lists the same names.
HAIR = ['Buzz cut', 'Short', 'Quiff', 'Long', 'Ponytail', 'Afro', 'Bun']
TOPS = ['T-shirt', 'Long sleeve', 'Hoodie', 'Tank top', 'Jacket', 'Suit']
PANTS = ['Jeans', 'Shorts', 'Cargo', 'Skirt']
SHOES = ['Sneakers', 'Boots', 'Dress shoes']
STYLES = {'hair': HAIR, 'top': TOPS, 'pants': PANTS, 'shoes': SHOES}

# Fixed colours (materials that keep their colour).
FIXED = {
    'eyes': (0.05, 0.05, 0.06),
    'white': (0.92, 0.92, 0.9),
    'dark': (0.08, 0.08, 0.1),
    'sole': (0.85, 0.85, 0.82),
    'lips': (0.62, 0.36, 0.34),
}
# Default look of a slot material in the exported file (SARE multiplies the
# player's colour in, so these are white).
SLOTS = ['skin', 'hair', 'top', 'pants', 'shoes']


def args():
    argv = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []
    opts = {'out': os.path.join(ROOT, 'content/sare-rp-town/characters.glb'),
            'blend': None, 'render': None, 'export': True}
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


# --- Proportions ---------------------------------------------------------------
class Body:
    def __init__(self, kind):
        self.kind = kind
        f = kind == 'F'
        s = 0.95 if f else 1.0
        self.z = lambda v: v * s
        self.shoulder = 0.168 if f else 0.19
        self.hip = 0.095 if f else 0.09
        self.elbow = 0.205 if f else 0.23
        self.wrist = 0.225 if f else 0.25
        self.chest_r = (0.128, 0.148) if f else (0.14, 0.172)
        self.pelvis_r = (0.168, 0.135) if f else (0.152, 0.142)
        self.thigh_r = (0.08, 0.056) if f else (0.076, 0.055)
        self.arm_r = (0.042, 0.034) if f else (0.048, 0.04)
        self.head_r = (0.088, 0.103, 0.115) if f else (0.094, 0.108, 0.12)
        z = self.z
        self.j = {
            'hip': z(0.92), 'waist': z(1.10), 'shoulder': z(1.41), 'neck': z(1.47),
            'head': z(1.62), 'top': z(1.75), 'elbow': z(1.14), 'wrist': z(0.885),
            'knee': z(0.50), 'ankle': z(0.09),
        }


# --- Mesh helpers ----------------------------------------------------------------
def new_object(name, bm, material):
    mesh = bpy.data.meshes.new(name)
    bm.normal_update()
    bm.to_mesh(mesh)
    bm.free()
    mesh.materials.append(material)
    for poly in mesh.polygons:
        poly.use_smooth = False
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    return obj


def limb_bm(p0, p1, r0, r1, segs=8, depth_scale=1.0, caps=True):
    p0, p1 = Vector(p0), Vector(p1)
    bm = bmesh.new()
    axis = p1 - p0
    length = axis.length
    bmesh.ops.create_cone(bm, cap_ends=caps, cap_tris=False, segments=segs,
                          radius1=r0, radius2=r1, depth=length)
    bmesh.ops.scale(bm, vec=(1.0, depth_scale, 1.0), verts=bm.verts)
    rot = Vector((0, 0, 1)).rotation_difference(axis.normalized()).to_matrix().to_4x4()
    bmesh.ops.transform(bm, matrix=Matrix.Translation((p0 + p1) / 2) @ rot, verts=bm.verts)
    return bm


def box_bm(center, size):
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    bmesh.ops.scale(bm, vec=size, verts=bm.verts)
    bmesh.ops.translate(bm, vec=center, verts=bm.verts)
    return bm


def sphere_bm(center, radii, segs=10, rings=7, keep=None):
    bm = bmesh.new()
    bmesh.ops.create_uvsphere(bm, u_segments=segs, v_segments=rings, radius=1.0)
    if keep:
        doomed = [f for f in bm.faces if not keep(f.calc_center_median())]
        bmesh.ops.delete(bm, geom=doomed, context='FACES')
    bmesh.ops.scale(bm, vec=radii, verts=bm.verts)
    bmesh.ops.translate(bm, vec=center, verts=bm.verts)
    return bm


class Builder:
    """Makes the pieces of one body and parents them to its bones."""

    def __init__(self, body, rig, materials, include):
        self.b = body
        self.rig = rig
        self.mats = materials
        self.include = include
        self.objects = []

    def piece(self, slot, style, part, bm, material, bone):
        if not self.include(slot, style):
            bm.free()
            return
        name = f'{self.b.kind}.{slot}.{style}.{part}'
        obj = new_object(name, bm, self.mats[material])
        world = obj.matrix_world.copy()
        obj.parent = self.rig
        obj.parent_type = 'BONE'
        obj.parent_bone = bone
        bpy.context.view_layer.update()
        obj.matrix_world = world
        self.objects.append(obj)

    def mirrored(self, fn):
        for side, sx in (('L', 1.0), ('R', -1.0)):
            fn(side, sx)


# --- Skeleton ----------------------------------------------------------------------
def bone_layout(b):
    j = b.j
    layout = {
        'hips': ((0, 0, j['hip']), (0, 0, j['waist']), None),
        'chest': ((0, 0, j['waist']), (0, 0, j['shoulder'] + 0.02), 'hips'),
        'neck': ((0, 0, j['shoulder'] + 0.02), (0, 0, j['neck'] + 0.06), 'chest'),
        'head': ((0, 0, j['neck'] + 0.06), (0, 0, j['top']), 'neck'),
    }
    for side, sx in (('L', 1.0), ('R', -1.0)):
        layout[f'upperarm.{side}'] = ((sx * b.shoulder, 0, j['shoulder']), (sx * b.elbow, 0, j['elbow']), 'chest')
        layout[f'forearm.{side}'] = ((sx * b.elbow, 0, j['elbow']), (sx * b.wrist, -0.02, j['wrist']), f'upperarm.{side}')
        layout[f'hand.{side}'] = ((sx * b.wrist, -0.02, j['wrist']), (sx * (b.wrist + 0.005), -0.02, j['wrist'] - 0.1), f'forearm.{side}')
        layout[f'thigh.{side}'] = ((sx * b.hip, 0, j['hip']), (sx * (b.hip + 0.008), 0, j['knee']), 'hips')
        layout[f'shin.{side}'] = ((sx * (b.hip + 0.008), 0, j['knee']), (sx * (b.hip + 0.008), 0.0, j['ankle']), f'thigh.{side}')
        layout[f'foot.{side}'] = ((sx * (b.hip + 0.008), 0.0, j['ankle']), (sx * (b.hip + 0.008), -0.14, 0.03), f'shin.{side}')
    return layout


def make_rig(b, name=None):
    arm = bpy.data.armatures.new(f'{b.kind}_rig')
    rig = bpy.data.objects.new(name or f'{b.kind}_rig', arm)
    bpy.context.scene.collection.objects.link(rig)
    bpy.context.view_layer.objects.active = rig
    bpy.ops.object.mode_set(mode='EDIT')
    bones = {}
    for name_, (head, tail, parent) in bone_layout(b).items():
        bone = arm.edit_bones.new(name_)
        bone.head, bone.tail = head, tail
        bone.roll = 0.0
        if parent:
            bone.parent = bones[parent]
        bones[name_] = bone
    bpy.ops.object.mode_set(mode='OBJECT')
    for pb in rig.pose.bones:
        pb.rotation_mode = 'QUATERNION'
    return rig


# --- Bodies, hair and clothes ----------------------------------------------------------
def build_body(bl):
    b, j = bl.b, bl.b.j
    f = b.kind == 'F'
    hr = b.head_r
    head_c = (0, 0, j['head'])
    bl.piece('base', 0, 'head', sphere_bm(head_c, hr, 12, 8), 'skin', 'head')
    bl.piece('base', 0, 'nose', box_bm((0, -hr[1] + 0.004, j['head'] - 0.012), (0.022, 0.03, 0.04)), 'skin', 'head')
    bl.piece('base', 0, 'jaw', box_bm((0, -0.035, j['head'] - 0.075), (hr[0] * 1.3, 0.09, 0.05)), 'skin', 'head')
    for sx in (1, -1):
        bl.piece('base', 0, f'eye{sx}', box_bm((sx * 0.034, -hr[1] + 0.01, j['head'] + 0.018), (0.024, 0.012, 0.016)), 'eyes', 'head')
        bl.piece('base', 0, f'brow{sx}', box_bm((sx * 0.034, -hr[1] + 0.008, j['head'] + 0.04), (0.03, 0.012, 0.008)), 'hair', 'head')
        bl.piece('base', 0, f'ear{sx}', box_bm((sx * hr[0], 0.005, j['head']), (0.02, 0.035, 0.045)), 'skin', 'head')
    bl.piece('base', 0, 'mouth', box_bm((0, -hr[1] + 0.01, j['head'] - 0.045), (0.035, 0.01, 0.008)), 'lips', 'head')
    bl.piece('base', 0, 'neck', limb_bm((0, 0.005, j['neck'] - 0.04), (0, 0.005, j['head'] - 0.06), 0.047, 0.043), 'skin', 'neck')
    cr, pr = b.chest_r, b.pelvis_r
    bl.piece('base', 0, 'chest', limb_bm((0, 0, j['waist'] - 0.02), (0, 0, j['shoulder'] + 0.035), cr[0], cr[1], 10, 0.62), 'skin', 'chest')
    bl.piece('base', 0, 'pelvis', limb_bm((0, 0, j['hip'] - 0.06), (0, 0, j['waist'] + 0.01), pr[0], pr[1], 10, 0.66), 'skin', 'hips')
    if f:
        for sx in (1, -1):
            bl.piece('base', 0, f'bust{sx}', sphere_bm((sx * 0.055, -0.068, j['shoulder'] - 0.12), (0.058, 0.05, 0.055), 8, 6), 'skin', 'chest')

    def limbs(side, sx):
        ar = b.arm_r
        sh = Vector((sx * b.shoulder, 0, j['shoulder']))
        el = Vector((sx * b.elbow, 0, j['elbow']))
        wr = Vector((sx * b.wrist, -0.02, j['wrist']))
        bl.piece('base', 0, f'shoulder.{side}', sphere_bm(sh, (0.056, 0.05, 0.05), 8, 6), 'skin', 'chest')
        bl.piece('base', 0, f'upperarm.{side}', limb_bm(sh, el, ar[0], ar[0] * 0.88), 'skin', f'upperarm.{side}')
        bl.piece('base', 0, f'forearm.{side}', limb_bm(el, wr, ar[1], ar[1] * 0.82), 'skin', f'forearm.{side}')
        bl.piece('base', 0, f'hand.{side}', box_bm(wr + Vector((sx * 0.003, 0.0, -0.052)), (0.03, 0.068, 0.098)), 'skin', f'hand.{side}')
        tr = b.thigh_r
        hip = Vector((sx * b.hip, 0, j['hip']))
        knee = Vector((sx * (b.hip + 0.008), 0, j['knee']))
        ankle = Vector((sx * (b.hip + 0.008), 0, j['ankle']))
        bl.piece('base', 0, f'thigh.{side}', limb_bm(hip, knee, tr[0], tr[1]), 'skin', f'thigh.{side}')
        bl.piece('base', 0, f'shin.{side}', limb_bm(knee, ankle, tr[1] * 0.95, 0.038), 'skin', f'shin.{side}')
        bl.piece('base', 0, f'foot.{side}', box_bm((ankle.x, -0.045, 0.04), (0.085, 0.2, 0.08)), 'skin', f'foot.{side}')
    bl.mirrored(limbs)


def build_hair(bl):
    b, j = bl.b, bl.b.j
    hr = b.head_r
    c = Vector((0, 0.006, j['head'] + 0.012))
    cap = lambda p: p.z > -0.15 and not (p.y < -0.55 and p.z < 0.42)
    # 0 buzz cut: a thin cap close to the head.
    bl.piece('hair', 0, 'cap', sphere_bm(c, (hr[0] + 0.004, hr[1] + 0.004, hr[2] + 0.002), 12, 8, cap), 'hair', 'head')
    # 1 short
    bl.piece('hair', 1, 'cap', sphere_bm(c + Vector((0, 0.004, 0.006)), (hr[0] + 0.014, hr[1] + 0.014, hr[2] + 0.014), 12, 8, cap), 'hair', 'head')
    # 2 quiff: short with volume at the front.
    bl.piece('hair', 2, 'cap', sphere_bm(c + Vector((0, 0.004, 0.006)), (hr[0] + 0.014, hr[1] + 0.014, hr[2] + 0.014), 12, 8, cap), 'hair', 'head')
    bl.piece('hair', 2, 'quiff', box_bm((0, -hr[1] * 0.55, j['head'] + hr[2] + 0.005), (hr[0] * 1.5, 0.08, 0.05)), 'hair', 'head')
    # 3 long: cap plus hair down the back to the shoulders.
    long_cap = lambda p: p.z > -0.35 and not (p.y < -0.55 and p.z < 0.42)
    bl.piece('hair', 3, 'cap', sphere_bm(c + Vector((0, 0.006, 0.004)), (hr[0] + 0.018, hr[1] + 0.016, hr[2] + 0.014), 12, 8, long_cap), 'hair', 'head')
    bl.piece('hair', 3, 'back', box_bm((0, 0.05, j['head'] - 0.1), (hr[0] * 2.05, 0.07, 0.2)), 'hair', 'head')
    for sx in (1, -1):
        bl.piece('hair', 3, f'side{sx}', box_bm((sx * (hr[0] + 0.004), 0.0, j['head'] - 0.07), (0.03, 0.1, 0.16)), 'hair', 'head')
    # 4 ponytail
    bl.piece('hair', 4, 'cap', sphere_bm(c + Vector((0, 0.004, 0.006)), (hr[0] + 0.014, hr[1] + 0.014, hr[2] + 0.014), 12, 8, cap), 'hair', 'head')
    bl.piece('hair', 4, 'tie', sphere_bm((0, hr[1] + 0.01, j['head'] + 0.02), (0.03, 0.03, 0.03), 8, 6), 'hair', 'head')
    bl.piece('hair', 4, 'tail', limb_bm((0, hr[1] + 0.025, j['head'] + 0.015), (0, hr[1] + 0.06, j['head'] - 0.17), 0.032, 0.018, 6), 'hair', 'head')
    # 5 afro: a big round puff.
    afro = lambda p: not (p.y < -0.45 and p.z < 0.38) and p.z > -0.3
    bl.piece('hair', 5, 'puff', sphere_bm(c + Vector((0, 0.012, 0.035)), (hr[0] + 0.05, hr[1] + 0.045, hr[2] + 0.045), 12, 8, afro), 'hair', 'head')
    # 6 bun
    bl.piece('hair', 6, 'cap', sphere_bm(c + Vector((0, 0.004, 0.006)), (hr[0] + 0.012, hr[1] + 0.012, hr[2] + 0.012), 12, 8, cap), 'hair', 'head')
    bl.piece('hair', 6, 'bun', sphere_bm((0, hr[1] * 0.55, j['head'] + hr[2] + 0.02), (0.045, 0.045, 0.04), 8, 6), 'hair', 'head')


def build_tops(bl):
    b, j = bl.b, bl.b.j
    f = b.kind == 'F'
    cr = b.chest_r

    def torso(style, grow=0.013, low=-0.05, mat='top'):
        bl.piece('top', style, 'torso', limb_bm((0, 0, j['waist'] + low), (0, 0, j['shoulder'] + 0.04), cr[0] + grow, cr[1] + grow, 10, 0.64), mat, 'chest')
        if f:
            for sx in (1, -1):
                bl.piece('top', style, f'bust{sx}', sphere_bm((sx * 0.055, -0.07, j['shoulder'] - 0.12), (0.058 + grow, 0.05 + grow, 0.055 + grow), 8, 6), mat, 'chest')

    def sleeves(style, length, grow=0.012, mat='top'):
        def one(side, sx):
            ar = b.arm_r
            sh = Vector((sx * b.shoulder, 0, j['shoulder']))
            el = Vector((sx * b.elbow, 0, j['elbow']))
            wr = Vector((sx * b.wrist, -0.02, j['wrist']))
            bl.piece('top', style, f'shoulder.{side}', sphere_bm(sh, (0.056 + grow, 0.05 + grow, 0.05 + grow), 8, 6), mat, 'chest')
            if length <= 1.0:
                bl.piece('top', style, f'upperarm.{side}', limb_bm(sh, sh.lerp(el, length), ar[0] + grow, ar[0] * 0.94 + grow), mat, f'upperarm.{side}')
            else:
                bl.piece('top', style, f'upperarm.{side}', limb_bm(sh, el, ar[0] + grow, ar[0] * 0.88 + grow), mat, f'upperarm.{side}')
                bl.piece('top', style, f'forearm.{side}', limb_bm(el, el.lerp(wr, length - 1.0), ar[1] + grow, ar[1] * 0.85 + grow), mat, f'forearm.{side}')
        bl.mirrored(one)

    # 0 t-shirt
    torso(0)
    sleeves(0, 0.5)
    # 1 long sleeve
    torso(1)
    sleeves(1, 1.95)
    # 2 hoodie: long sleeves, hood and a front pocket.
    torso(2, 0.018, -0.07)
    sleeves(2, 1.95, 0.016)
    hood = lambda p: p.y > -0.2
    bl.piece('top', 2, 'hood', sphere_bm((0, 0.07, j['neck'] + 0.02), (0.13, 0.08, 0.075), 10, 6, hood), 'top', 'chest')
    bl.piece('top', 2, 'pocket', box_bm((0, -cr[0] * 0.62 - 0.012, j['waist'] + 0.02), (0.17, 0.02, 0.09)), 'top', 'chest')
    for sx in (1, -1):
        bl.piece('top', 2, f'string{sx}', box_bm((sx * 0.03, -cr[1] * 0.62 - 0.01, j['shoulder'] - 0.06), (0.008, 0.008, 0.09)), 'white', 'chest')
    # 3 tank top: no sleeves, a lower neckline.
    bl.piece('top', 3, 'torso', limb_bm((0, 0, j['waist'] - 0.05), (0, 0, j['shoulder'] - 0.02), cr[0] + 0.013, cr[1] + 0.006, 10, 0.64), 'top', 'chest')
    if f:
        for sx in (1, -1):
            bl.piece('top', 3, f'bust{sx}', sphere_bm((sx * 0.055, -0.07, j['shoulder'] - 0.12), (0.071, 0.063, 0.068), 8, 6), 'top', 'chest')
    for sx in (1, -1):
        bl.piece('top', 3, f'strap{sx}', box_bm((sx * 0.08, 0, j['shoulder'] + 0.01), (0.035, cr[1] * 1.3, 0.03)), 'top', 'chest')
    # 4 jacket over a white shirt: open front, collar.
    torso(4, 0.024, -0.09)
    sleeves(4, 1.98, 0.02)
    bl.piece('top', 4, 'shirt', box_bm((0, -cr[1] * 0.64 - 0.016, j['shoulder'] - 0.12), (0.075, 0.012, 0.26)), 'white', 'chest')
    for sx in (1, -1):
        bl.piece('top', 4, f'collar{sx}', box_bm((sx * 0.07, -0.04, j['shoulder'] + 0.035), (0.06, 0.12, 0.04)), 'top', 'chest')
    # 5 suit jacket with white shirt and tie.
    torso(5, 0.022, -0.1)
    sleeves(5, 1.98, 0.019)
    bl.piece('top', 5, 'shirt', box_bm((0, -cr[1] * 0.64 - 0.014, j['shoulder'] - 0.1), (0.07, 0.012, 0.24)), 'white', 'chest')
    bl.piece('top', 5, 'tie', box_bm((0, -cr[1] * 0.64 - 0.022, j['shoulder'] - 0.12), (0.03, 0.01, 0.2)), 'dark', 'chest')
    for sx in (1, -1):
        bl.piece('top', 5, f'lapel{sx}', box_bm((sx * 0.055, -cr[1] * 0.62 - 0.02, j['shoulder'] - 0.04), (0.04, 0.012, 0.12)), 'top', 'chest')


def build_pants(bl):
    b, j = bl.b, bl.b.j
    pr = b.pelvis_r

    def hips(style, grow=0.012):
        bl.piece('pants', style, 'pelvis', limb_bm((0, 0, j['hip'] - 0.075), (0, 0, j['waist'] + 0.0), pr[0] + grow, pr[1] + grow, 10, 0.68), 'pants', 'hips')

    def legs(style, length, grow=0.012, cuff=False):
        def one(side, sx):
            tr = b.thigh_r
            hip = Vector((sx * b.hip, 0, j['hip']))
            knee = Vector((sx * (b.hip + 0.008), 0, j['knee']))
            ankle = Vector((sx * (b.hip + 0.008), 0, j['ankle'] + 0.02))
            if length <= 1.0:
                bl.piece('pants', style, f'thigh.{side}', limb_bm(hip, hip.lerp(knee, length), tr[0] + grow, tr[0] * 0.85 + grow), 'pants', f'thigh.{side}')
            else:
                bl.piece('pants', style, f'thigh.{side}', limb_bm(hip, knee, tr[0] + grow, tr[1] + grow), 'pants', f'thigh.{side}')
                bl.piece('pants', style, f'shin.{side}', limb_bm(knee, knee.lerp(ankle, length - 1.0), tr[1] * 0.95 + grow, 0.045 + grow), 'pants', f'shin.{side}')
        bl.mirrored(one)

    hips(0)
    legs(0, 2.0)
    hips(1)
    legs(1, 0.6, 0.016)
    hips(2)
    legs(2, 2.0, 0.018)
    for sx, side in ((1, 'L'), (-1, 'R')):
        bl.piece('pants', 2, f'pocket.{side}', box_bm((sx * (b.hip + b.thigh_r[0] + 0.02), 0, j['knee'] + 0.2), (0.03, 0.09, 0.1)), 'pants', f'thigh.{side}')
    hips(3, 0.014)
    bl.piece('pants', 3, 'skirt', limb_bm((0, 0, j['hip'] + 0.02), (0, 0, j['knee'] + 0.05), pr[0] + 0.02, pr[0] + 0.09, 12, 0.78, caps=False), 'pants', 'hips')


def build_shoes(bl):
    b, j = bl.b, bl.b.j

    def one(side, sx):
        x = sx * (b.hip + 0.008)
        bone = f'foot.{side}'
        # 0 sneakers: white sole.
        bl.piece('shoes', 0, f'upper.{side}', box_bm((x, -0.048, 0.055), (0.1, 0.215, 0.07)), 'shoes', bone)
        bl.piece('shoes', 0, f'sole.{side}', box_bm((x, -0.048, 0.012), (0.104, 0.22, 0.026)), 'sole', bone)
        # 1 boots: up the shin.
        bl.piece('shoes', 1, f'upper.{side}', box_bm((x, -0.05, 0.05), (0.102, 0.22, 0.09)), 'shoes', bone)
        bl.piece('shoes', 1, f'shaft.{side}', limb_bm((x, 0, j['ankle'] - 0.03), (x, 0, j['ankle'] + 0.17), 0.055, 0.052), 'shoes', f'shin.{side}')
        bl.piece('shoes', 1, f'sole.{side}', box_bm((x, -0.05, 0.01), (0.106, 0.225, 0.022)), 'dark', bone)
        # 2 dress shoes: slim, dark sole.
        bl.piece('shoes', 2, f'upper.{side}', box_bm((x, -0.05, 0.045), (0.092, 0.215, 0.06)), 'shoes', bone)
        bl.piece('shoes', 2, f'sole.{side}', box_bm((x, -0.05, 0.008), (0.095, 0.22, 0.016)), 'dark', bone)
    bl.mirrored(one)


def build_character(kind, materials, include=lambda slot, style: True, name=None):
    b = Body(kind)
    rig = make_rig(b, name)
    bl = Builder(b, rig, materials, include)
    build_body(bl)
    build_hair(bl)
    build_tops(bl)
    build_pants(bl)
    build_shoes(bl)
    return rig, bl.objects


# --- Animation ------------------------------------------------------------------------
X, Y, Z = Vector((1, 0, 0)), Vector((0, 1, 0)), Vector((0, 0, 1))


def deg(a):
    return math.radians(a)


def pose_frame(t, clip):
    """World-axis rotations (axis, degrees) per bone and a hip lift, at
    phase t in [0, 1) of a looping clip. Facing -Y: a negative angle about
    X swings a hanging limb forwards."""
    s = math.sin(2 * math.pi * t)
    c = math.cos(2 * math.pi * t)
    r = {}
    lift = 0.0
    if clip == 'idle':
        breathe = math.sin(2 * math.pi * t)
        r['chest'] = [(X, -1.5 * breathe)]
        r['head'] = [(Z, 3 * math.sin(2 * math.pi * t + 1.0)), (X, 1.0)]
        for side, sx in (('L', 1), ('R', -1)):
            r[f'upperarm.{side}'] = [(Y, -sx * 4.0), (X, 2 * breathe)]
            r[f'forearm.{side}'] = [(X, -10)]
            r[f'thigh.{side}'] = [(Y, -sx * 1.5)]
        lift = -0.004 * breathe
    elif clip == 'walk':
        for side, sx, ph in (('L', 1, 0.0), ('R', -1, 0.5)):
            sp = math.sin(2 * math.pi * (t + ph))
            swing = math.sin(2 * math.pi * (t + ph + 0.15))
            r[f'thigh.{side}'] = [(X, -24 * sp)]
            r[f'shin.{side}'] = [(X, 6 + 34 * max(0.0, swing))]
            r[f'foot.{side}'] = [(X, -8 * sp)]
            r[f'upperarm.{side}'] = [(Y, -sx * 5.0), (X, 18 * sp)]
            r[f'forearm.{side}'] = [(X, -14 - 10 * max(0.0, -sp))]
        r['chest'] = [(Z, 5 * s), (X, -2)]
        r['hips'] = [(Z, -4 * s)]
        lift = 0.018 * abs(c) - 0.012
    elif clip == 'run':
        for side, sx, ph in (('L', 1, 0.0), ('R', -1, 0.5)):
            sp = math.sin(2 * math.pi * (t + ph))
            swing = math.sin(2 * math.pi * (t + ph + 0.2))
            r[f'thigh.{side}'] = [(X, -6 - 40 * sp)]
            r[f'shin.{side}'] = [(X, 20 + 70 * max(0.0, swing))]
            r[f'foot.{side}'] = [(X, -10 * sp)]
            r[f'upperarm.{side}'] = [(Y, -sx * 8.0), (X, 38 * sp)]
            r[f'forearm.{side}'] = [(X, -62)]
        r['chest'] = [(Z, 8 * s), (X, -10)]
        r['hips'] = [(Z, -6 * s), (X, -4)]
        r['head'] = [(X, 8)]
        lift = 0.05 * abs(c) - 0.04
    elif clip == 'sit':
        for side, sx in (('L', 1), ('R', -1)):
            r[f'thigh.{side}'] = [(X, -88), (Y, -sx * 4)]
            r[f'shin.{side}'] = [(X, 82)]
            r[f'foot.{side}'] = [(X, -6)]
            r[f'upperarm.{side}'] = [(X, -42), (Y, sx * 6)]
            r[f'forearm.{side}'] = [(X, -38)]
        r['chest'] = [(X, 6)]
        r['head'] = [(X, -4)]
        lift = -0.42
    elif clip == 'fall':
        for side, sx in (('L', 1), ('R', -1)):
            r[f'upperarm.{side}'] = [(Y, -sx * (55 + 8 * s))]
            r[f'forearm.{side}'] = [(X, -25)]
            r[f'thigh.{side}'] = [(X, -25 if sx > 0 else 10)]
            r[f'shin.{side}'] = [(X, 40 if sx > 0 else 25)]
        r['chest'] = [(X, 4)]
    return r, lift


CLIPS = {'idle': 2.5, 'walk': 1.0, 'run': 0.6, 'sit': 1.0, 'fall': 1.0}


def animate(rigs):
    scene = bpy.context.scene
    scene.render.fps = FPS
    for rig in rigs:
        rig.animation_data_create()
    for clip, seconds in CLIPS.items():
        frames = int(round(seconds * FPS))
        action = bpy.data.actions.new(clip)
        action.use_fake_user = True
        for rig in rigs:
            rig.animation_data.action = action
            rest = {pb.name: pb.bone.matrix_local.to_3x3() for pb in rig.pose.bones}
            for frame in range(frames + 1):
                rotations, lift = pose_frame((frame % frames) / frames, clip)
                for pb in rig.pose.bones:
                    q = Quaternion()
                    for axis, angle in rotations.get(pb.name, []):
                        q = Quaternion(axis, deg(angle)) @ q
                    r = rest[pb.name]
                    # A world-axis turn at the bone's head, in the bone's own axes.
                    local = (r.inverted() @ q.to_matrix() @ r).to_quaternion()
                    pb.rotation_quaternion = local
                    pb.keyframe_insert('rotation_quaternion', frame=frame)
                    if pb.name == 'hips':
                        # The hips bone points up: its local Y is world Z.
                        pb.location = (0.0, lift, 0.0)
                        pb.keyframe_insert('location', frame=frame)
                    else:
                        pb.location = (0.0, 0.0, 0.0)
        for fcurve in action.fcurves:
            for key in fcurve.keyframe_points:
                key.interpolation = 'LINEAR'
    # One NLA track per clip on every rig: the exporter merges tracks with
    # the same name into one glTF animation that moves both skeletons.
    for rig in rigs:
        rig.animation_data.action = None
        for clip in CLIPS:
            track = rig.animation_data.nla_tracks.new()
            track.name = clip
            action = bpy.data.actions[clip]
            track.strips.new(clip, int(action.frame_range[0]), action)
            track.mute = True
        for pb in rig.pose.bones:
            pb.rotation_quaternion = Quaternion()
            pb.location = (0, 0, 0)


def set_pose(rig, clip, t):
    rotations, lift = pose_frame(t, clip)
    rig.animation_data_clear()
    for pb in rig.pose.bones:
        q = Quaternion()
        for axis, angle in rotations.get(pb.name, []):
            q = Quaternion(axis, deg(angle)) @ q
        r = pb.bone.matrix_local.to_3x3()
        pb.rotation_quaternion = (r.inverted() @ q.to_matrix() @ r).to_quaternion()
        pb.location = (0.0, lift if pb.name == 'hips' else 0.0, 0.0)


# --- Materials ----------------------------------------------------------------------------
def flat_material(name, rgb):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes['Principled BSDF']
    bsdf.inputs['Base Color'].default_value = (*rgb, 1.0)
    bsdf.inputs['Roughness'].default_value = 0.8
    mat.diffuse_color = (*rgb, 1.0)
    return mat


def export_materials():
    mats = {name: flat_material(name, (1.0, 1.0, 1.0)) for name in SLOTS}
    mats.update({name: flat_material(name, rgb) for name, rgb in FIXED.items()})
    return mats


def look_materials(prefix, look):
    mats = {name: flat_material(f'{prefix}_{name}', rgb) for name, rgb in FIXED.items()}
    for slot in SLOTS:
        mats[slot] = flat_material(f'{prefix}_{slot}', srgb(look[slot + '_color']))
    return mats


def srgb(rgb):
    """0-255 sRGB to Blender's linear colour."""
    def lin(c):
        c /= 255.0
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4
    return tuple(lin(float(c)) for c in rgb)


def make_look(kind, look, name, location=(0, 0, 0), heading=0.0, clip='idle', t=0.0):
    """One dressed character for renders: only the chosen styles."""
    mats = look_materials(name, look)
    chosen = lambda slot, style: slot == 'base' or look.get(slot) == style
    rig, objects = build_character(kind, mats, chosen, name=name)
    set_pose(rig, clip, t)
    rig.location = location
    rig.rotation_euler = (0, 0, heading)
    return rig


# A few looks for the line-up: (body, styles, colours).
LINEUP = [
    ('M', dict(hair=1, top=0, pants=0, shoes=0, skin_color=(224, 172, 140), hair_color=(60, 40, 28), top_color=(200, 40, 40), pants_color=(50, 70, 120), shoes_color=(240, 240, 240)), 'idle', 0.0),
    ('F', dict(hair=4, top=1, pants=3, shoes=2, skin_color=(198, 134, 96), hair_color=(30, 22, 18), top_color=(240, 200, 60), pants_color=(40, 40, 50), shoes_color=(120, 30, 40)), 'walk', 0.25),
    ('M', dict(hair=5, top=2, pants=2, shoes=1, skin_color=(120, 78, 52), hair_color=(20, 16, 14), top_color=(70, 140, 70), pants_color=(110, 100, 70), shoes_color=(90, 60, 40)), 'idle', 0.4),
    ('F', dict(hair=3, top=4, pants=0, shoes=1, skin_color=(240, 200, 170), hair_color=(200, 150, 70), top_color=(40, 40, 46), pants_color=(70, 90, 140), shoes_color=(60, 40, 30)), 'idle', 0.7),
    ('M', dict(hair=2, top=5, pants=0, shoes=2, skin_color=(170, 112, 78), hair_color=(40, 30, 24), top_color=(36, 40, 60), pants_color=(36, 40, 60), shoes_color=(20, 20, 20)), 'idle', 0.1),
    ('F', dict(hair=6, top=3, pants=1, shoes=0, skin_color=(90, 58, 40), hair_color=(30, 20, 16), top_color=(60, 170, 200), pants_color=(220, 220, 210), shoes_color=(250, 120, 160)), 'walk', 0.75),
    ('M', dict(hair=0, top=3, pants=1, shoes=0, skin_color=(250, 214, 190), hair_color=(150, 60, 30), top_color=(250, 250, 250), pants_color=(200, 90, 30), shoes_color=(40, 40, 40)), 'run', 0.3),
    ('F', dict(hair=1, top=2, pants=2, shoes=1, skin_color=(214, 160, 120), hair_color=(160, 40, 90), top_color=(140, 60, 160), pants_color=(60, 60, 60), shoes_color=(240, 240, 240)), 'idle', 0.9),
]


def render_lineup(path):
    for obj in list(bpy.data.objects):
        bpy.data.objects.remove(obj, do_unlink=True)
    for i, (kind, look, clip, t) in enumerate(LINEUP):
        make_look(kind, look, f'look{i}', location=((i - 3.5) * 0.78, 0, 0), heading=math.radians(-8 + 4 * (i % 3)), clip=clip, t=t)
    scene = bpy.context.scene
    ground = bpy.data.meshes.new('ground')
    bm = bmesh.new()
    bmesh.ops.create_grid(bm, x_segments=1, y_segments=1, size=12)
    bm.to_mesh(ground)
    bm.free()
    g = bpy.data.objects.new('ground', ground)
    g.data.materials.append(flat_material('ground_mat', (0.55, 0.57, 0.6)))
    scene.collection.objects.link(g)
    cam_data = bpy.data.cameras.new('cam')
    cam_data.lens = 42
    cam = bpy.data.objects.new('cam', cam_data)
    scene.collection.objects.link(cam)
    cam.location = (0.0, -8.6, 1.45)
    cam.rotation_euler = (math.radians(86), 0, 0)
    scene.camera = cam
    sun_data = bpy.data.lights.new('sun', 'SUN')
    sun_data.energy = 3.5
    sun_data.angle = math.radians(8)
    sun = bpy.data.objects.new('sun', sun_data)
    sun.rotation_euler = (math.radians(50), math.radians(10), math.radians(-35))
    scene.collection.objects.link(sun)
    setup_cycles(scene, path, 1800, 900)
    bpy.ops.render.render(write_still=True)
    print('rendered', path)


def setup_cycles(scene, path, w, h, samples=32):
    scene.render.engine = 'CYCLES'
    scene.cycles.device = 'CPU'
    scene.cycles.samples = samples
    scene.cycles.use_denoising = True
    scene.render.resolution_x, scene.render.resolution_y = w, h
    scene.render.filepath = path
    world = scene.world or bpy.data.worlds.new('World')
    scene.world = world
    world.use_nodes = True
    world.node_tree.nodes['Background'].inputs['Color'].default_value = (0.62, 0.75, 0.95, 1)
    world.node_tree.nodes['Background'].inputs['Strength'].default_value = 0.8
    scene.view_settings.view_transform = 'Standard'


def main():
    opts = args()
    for obj in list(bpy.data.objects):
        bpy.data.objects.remove(obj, do_unlink=True)
    mats = export_materials()
    rigs = []
    for kind in ('M', 'F'):
        rig, _ = build_character(kind, mats)
        rigs.append(rig)
    animate(rigs)
    if opts['export']:
        os.makedirs(os.path.dirname(opts['out']), exist_ok=True)
        bpy.ops.export_scene.gltf(filepath=opts['out'], export_format='GLB', export_yup=True,
                                  export_apply=True, export_normals=True, export_texcoords=False,
                                  export_vertex_color='NONE', export_cameras=False, export_lights=False,
                                  export_extras=False, export_skins=False, export_animations=True,
                                  export_animation_mode='NLA_TRACKS', export_force_sampling=True,
                                  export_frame_step=1, export_materials='EXPORT')
        print('wrote', opts['out'], os.path.getsize(opts['out']), 'bytes')
    if opts['blend']:
        bpy.ops.wm.save_as_mainfile(filepath=opts['blend'])
    if opts['render']:
        render_lineup(opts['render'])


if __name__ == '__main__':
    main()
