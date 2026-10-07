#!/usr/bin/env python3
"""Convert legacy GTA V resources or CodeWalker XML into a native SA resource.

Vehicles/props use the highest available drawable LOD. Players and clothing
require an explicit bone map and a native base player for bind-pose retargeting.
No FiveM Lua, GTA V game code, archives or escrow payloads are executed.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import struct as S
import subprocess
import tempfile
import xml.etree.ElementTree as ET

VERSION = 0x1803FFFF
LIMIT = 16 * 1024 * 1024
IDENTITY = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.]
SEMANTICS = {'Position': 3, 'BlendWeights': 4, 'BlendIndices': 4, 'Normal': 3,
             'Colour0': 4, 'Colour1': 4, 'Tangent': 4}
SEMANTICS.update({f'TexCoord{i}': 2 for i in range(8)})


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read(path, limit=LIMIT):
    require(path.stat().st_size <= limit, f'{path.name} exceeds {limit // 1048576} MiB')
    return path.read_bytes()


def chunk(tag, body):
    return S.pack('<III', tag, len(body), VERSION) + body


def chunks(data):
    offset = 0
    while offset < len(data):
        require(offset + 12 <= len(data), 'Truncated RenderWare header')
        tag, size, version = S.unpack_from('<III', data, offset)
        require(version == VERSION and offset + 12 + size <= len(data), 'Invalid RenderWare chunk')
        yield tag, data[offset + 12:offset + 12 + size]
        offset += 12 + size


def one(data, tag):
    items = [body for found, body in chunks(data) if found == tag]
    require(len(items) == 1, f'Expected one RenderWare section {tag}')
    return items[0]


def mul(a, b):
    return [sum(a[k * 4 + row] * b[col * 4 + k] for k in range(4))
            for col in range(4) for row in range(4)]


def inverse(matrix):
    rows = [[matrix[col * 4 + row] for col in range(4)] +
            [float(row == col) for col in range(4)] for row in range(4)]
    for col in range(4):
        pivot = max(range(col, 4), key=lambda row: abs(rows[row][col]))
        require(abs(rows[pivot][col]) > 1e-10, 'Singular bind matrix')
        rows[col], rows[pivot] = rows[pivot], rows[col]
        factor = rows[col][col]
        rows[col] = [v / factor for v in rows[col]]
        for row in range(4):
            if row != col:
                factor = rows[row][col]
                rows[row] = [a - factor * b for a, b in zip(rows[row], rows[col])]
    return [rows[row][col + 4] for col in range(4) for row in range(4)]


def transform(matrix, point, translate=True):
    return [sum(matrix[col * 4 + row] * point[col] for col in range(3)) +
            (matrix[12 + row] if translate else 0.) for row in range(3)]


def normal_transform(matrix, normal):
    # Normals transform by the inverse transpose, including nonuniform scales.
    inv = inverse(matrix)
    return [sum(inv[row * 4 + col] * normal[col] for col in range(3)) for row in range(3)]


def fragment_children(root, report):
    """Bake pristine physics children; shared car wheels are instanced by bone tag."""
    lod = root.find('Physics/LOD1')
    if lod is None:
        return []
    children = lod.findall('Children/Item')
    matrices = lod.findall('Transforms/Item')
    offset = vector(lod,'PositionOffset',[0.,0.,0.])
    shaders = root.findall('Drawable/ShaderGroup/Shaders/Item')
    wheel_tags = {27922,27902,26418,26398}
    wheels = [c.find('Drawable') for c in children if value(c,'BoneTag') in wheel_tags
              and c.find('Drawable/DrawableModelsHigh/Item/Geometries/Item') is not None]
    jobs = []
    for index, child in enumerate(children):
        drawable = child.find('Drawable')
        tag = value(child,'BoneTag')
        if drawable is None:
            continue
        if drawable.find('.//Geometries/Item') is None:
            if tag in wheel_tags and len(wheels) == 1:
                drawable = wheels[0]
            else:
                continue
        require(index < len(matrices), 'Missing pristine fragment child transform')
        matrix = [float(v) for v in (matrices[index].text or '').split()]
        require(len(matrix) == 16 and all(math.isfinite(v) for v in matrix), 'Invalid fragment transform')
        matrix[3] = matrix[7] = matrix[11] = 0.; matrix[15] = 1.
        for axis in range(3):
            matrix[12+axis] += offset[axis]
        if tag in (26418,26398):
            # GTA V instances the shared left wheel with a 180 degree Y rotation.
            for col, sign in [(0,-1),(1,1),(2,-1)]:
                for row in range(3):
                    matrix[col*4+row] = float(col == row) * sign
        jobs.append((drawable,matrix,shaders))
    if jobs:
        report['warnings'].append(f'Baked {len(jobs)} pristine physics children; wheels and doors remain static')
    return jobs


def value(node, name, default=0):
    child = node.find(name)
    return int(child.get('value', default)) if child is not None else default


def vector(node, name, defaults):
    child = node.find(name)
    return [float(child.get(axis, v)) if child is not None else v
            for axis, v in zip('xyzw', defaults)]


def skeleton(node):
    require(node is not None, 'Missing drawable skeleton source')
    bones = {}
    for bone in node.findall('Skeleton/Bones/Item'):
        index = value(bone, 'Index')
        require(index not in bones, 'Duplicate skeleton index')
        x, y, z, w = vector(bone, 'Rotation', [0., 0., 0., 1.])
        norm = math.sqrt(x*x + y*y + z*z + w*w)
        require(norm > 1e-8, 'Invalid skeleton quaternion')
        x, y, z, w = [v / norm for v in (x, y, z, w)]
        scale = vector(bone, 'Scale', [1., 1., 1.])
        local = [1-2*(y*y+z*z), 2*(x*y+z*w), 2*(x*z-y*w), 0.,
                 2*(x*y-z*w), 1-2*(x*x+z*z), 2*(y*z+x*w), 0.,
                 2*(x*z+y*w), 2*(y*z-x*w), 1-2*(x*x+y*y), 0.,
                 *vector(bone, 'Translation', [0., 0., 0.]), 1.]
        for col in range(3):
            for row in range(3):
                local[col * 4 + row] *= scale[col]
        bones[index] = {'tag': value(bone, 'Tag'), 'name': bone.findtext('Name', ''),
                        'parent': value(bone, 'ParentIndex', -1), 'local': local}
    require(len(bones) <= 256, 'Skeleton exceeds bone budget')
    def world(index, visiting):
        require(index in bones and index not in visiting, 'Missing or cyclic skeleton bone')
        bone = bones[index]
        if 'world' not in bone:
            bone['world'] = bone['local'] if bone['parent'] < 0 else mul(
                world(bone['parent'], visiting | {index}), bone['local'])
        return bone['world']
    for index in bones:
        world(index, set())
    return bones


def native_rig(path):
    clump = one(read(path), 16)
    frames = one(clump, 14)
    ids = None
    for tag, ext in chunks(frames):
        if tag != 3:
            continue
        for plugin, body in chunks(ext):
            if plugin == 0x11E:
                require(len(body) >= 12, 'Truncated HAnim header')
                count = S.unpack_from('<I', body, 8)[0]
                if count:
                    require(ids is None and count <= 255, 'Invalid native bone palette')
                    require(len(body) >= 20 + count * 12, 'Truncated HAnim bone palette')
                    ids = [None] * count
                    for n in range(count):
                        bone, index, flags = S.unpack_from('<iII', body, 20 + n * 12)
                        require(index < count and ids[index] is None, 'Invalid native bone index')
                        ids[index] = bone
    require(ids is not None, 'Base player has no HAnim skeleton')
    parts = [body for tag, body in chunks(one(clump, 26)) if tag == 15]
    require(parts, 'Base player has no geometry')
    geometry = parts[0]
    vertices = S.unpack_from('<I', one(geometry, 1), 8)[0]
    skin = one(one(geometry, 3), 0x116)
    require(len(skin) >= 4, 'Truncated native skin')
    bones, used, influences, pad = skin[:4]
    require(bones == len(ids), 'Base skin and hierarchy disagree')
    offset = 4 + used + vertices * 20
    binds = []
    for _ in ids:
        if used == 0:
            offset += 4
        binds.append(list(S.unpack_from('<16f', skin, offset)))
        offset += 64
    return frames, ids, binds


def geometry(vertices, indices, texture, skin=None, alpha=255, colour=(255,255,255)):
    require(0 < len(vertices) <= 65535 and 0 < len(indices) // 3 <= 200000,
            'Geometry exceeds native vertex/triangle limits')
    require(len(indices) % 3 == 0 and all(0 <= i < len(vertices) for i in indices), 'Invalid triangle indices')
    body = S.pack('<IIII', 0x1001E, len(indices) // 3, len(vertices), 1)
    body += bytes(c for v in vertices for c in v['colour'])
    body += b''.join(S.pack('<2f', *v['uv']) for v in vertices)
    body += b''.join(S.pack('<4H', indices[i+1], indices[i], 0, indices[i+2]) for i in range(0, len(indices), 3))
    body += S.pack('<4fII', 0., 0., 0., 100., 1, 1)
    body += b''.join(S.pack('<3f', *v['pos']) for v in vertices)
    body += b''.join(S.pack('<3f', *v['normal']) for v in vertices)
    material = chunk(1, S.pack('<I4BII3f', 0, *colour, alpha, 0, int(texture is not None), 1., 1., 1.))
    if texture:
        material += chunk(6, chunk(1, S.pack('<I', 0x1102)) + chunk(2, texture.encode('ascii') + b'\0') + chunk(2, b'\0') + chunk(3, b''))
    material += chunk(3, b'')
    materials = chunk(8, chunk(1, S.pack('<Ii', 1, -1)) + chunk(7, material))
    extension = b''
    if skin:
        palette, binds = skin
        extension = bytes([len(binds), len(binds), 4, 0]) + bytes(range(len(binds)))
        extension += bytes(i for v in vertices for i in v['bones'])
        extension += b''.join(S.pack('<4f', *v['weights']) for v in vertices)
        extension += b''.join(S.pack('<16f', *bind) for bind in binds) + bytes(12)
        extension = chunk(0x116, extension)
    return chunk(15, chunk(1, body) + materials + chunk(3, extension))


def dff(geometries, frames=None):
    require(0 < len(geometries) <= 128, 'Drawable exceeds native geometry count')
    if frames is None:
        frame = [IDENTITY[i] for i in (0,1,2,4,5,6,8,9,10,12,13,14)]
        frames = chunk(1, S.pack('<I12fiI', 1, *frame, -1, 0)) + chunk(3, b'')
    body = chunk(1, S.pack('<III', len(geometries), 0, 0)) + chunk(14, frames)
    body += chunk(26, chunk(1, S.pack('<I', len(geometries))) + b''.join(geometries))
    body += b''.join(chunk(20, chunk(1, S.pack('<IIII', 0, i, 5, 0)) + chunk(3, b'')) for i in range(len(geometries)))
    return chunk(16, body + chunk(3, b''))


def native_texture(name, data):
    require(len(data) >= 128 and data[:4] == b'DDS ', 'Expected DDS texture')
    height, width = S.unpack_from('<II', data, 12)
    require(0 < width <= 4096 and 0 < height <= 4096, 'DDS dimensions exceed native limits')
    flags, fourcc, bits, r, g, b, a = S.unpack_from('<7I', data, 80)
    if flags & 4:
        require(fourcc in [int.from_bytes(f, 'little') for f in (b'DXT1', b'DXT3', b'DXT5')], 'DDS requires DXT1/3/5 or 32-bit RGB; convert BC7/DX10 first')
        size = ((width + 3) // 4) * ((height + 3) // 4) * (8 if fourcc == int.from_bytes(b'DXT1', 'little') else 16)
        pixels = data[128:128 + size]
        format_ = fourcc
    else:
        require(bits == 32, 'Only 32-bit uncompressed DDS is supported')
        require((r,g,b,a) in [(0xff0000,0xff00,0xff,0xff000000), (0xff,0xff00,0xff0000,0xff000000), (0xff0000,0xff00,0xff,0)], 'Unsupported DDS channel masks')
        size = width * height * 4
        pixels = data[128:128 + size]
        require(len(pixels) == size, 'Truncated DDS pixels')
        if r == 0xff:
            pixels = bytes(c for i in range(0,len(pixels),4) for c in (pixels[i+2],pixels[i+1],pixels[i],pixels[i+3]))
        format_ = 21 if a else 22
    require(len(pixels) == size and name.isascii() and len(name.encode()) < 32, 'Truncated DDS or long native texture name')
    header = S.pack('<II', 9, 0x1102) + name.encode().ljust(32,b'\0') + bytes(32)
    header += S.pack('<IIHH4B', 0x500, format_, width, height, 32, 1, 4, 1)
    return chunk(21, chunk(1, header + S.pack('<I',size) + pixels) + chunk(3,b''))


def parse_xml(path):
    data = read(path, 128 * 1024 * 1024)
    require(b'\0' not in data, 'Only UTF-8 CodeWalker XML is supported')
    require(b'<!DOCTYPE' not in data.upper() and b'<!ENTITY' not in data.upper(), 'DTD/entity XML is unsupported')
    return ET.fromstring(data)


def extract(path, temporary):
    if path.suffix.lower() == '.xml':
        return path
    project = Path(__file__).parent / 'gta5-extract' / 'Gta5Extract.csproj'
    dll = project.parent / 'bin/Release/net9.0/Gta5Extract.dll'
    if not dll.exists():
        subprocess.run(['dotnet','build',str(project),'-c','Release'], check=True)
    subprocess.run(['dotnet',str(dll),str(path),str(temporary)], check=True)
    return temporary / (path.name + '.xml')


def convert(args):
    require(not args.out.exists(), 'Output already exists; choose a new resource directory')
    report = {'source': args.input.name, 'sha256': hashlib.sha256(read(args.input,128*1024*1024)).hexdigest(),
              'warnings': [], 'meshes': 0, 'vertices': 0, 'triangles': 0, 'textures': 0}
    with tempfile.TemporaryDirectory(prefix='sa-gta5-') as scratch:
        scratch = Path(scratch)
        source = extract(args.input, scratch/'model')
        root = parse_xml(source)
        roots = [root] if root.tag in ('Drawable','Item') else ([root.find('Drawable')] if root.tag == 'Fragment' else list(root.findall('Item')))
        require(roots and all(r is not None for r in roots), 'Expected Drawable, DrawableDictionary or Fragment XML')
        require(len(roots) <= 128, 'Drawable dictionary exceeds geometry budget')
        require(args.type != 'vehicle' or root.tag == 'Fragment', 'Vehicle conversion requires YFT Fragment XML')
        texture_paths = [source.parent]
        if args.textures:
            if args.textures.is_dir():
                texture_paths.insert(0,args.textures)
            else:
                texture_paths.insert(0,extract(args.textures,scratch/'textures').parent)
        images = {}
        for directory in texture_paths:
            for path in sorted(directory.rglob('*.dds')):
                images.setdefault(path.stem.lower(),path)
        frames = None
        rig = None
        if args.type in ('clothing','player'):
            require(args.base_player and args.base_ifp and args.bone_map,
                    'Skinned conversion needs --base-player native.dff, --base-ifp native.ifp and --bone-map map.json')
            frames, palette, binds = native_rig(args.base_player)
            mapping = json.loads(read(args.bone_map))
            require(isinstance(mapping,dict), 'Bone map must map GTA bone names/tags to native HAnim IDs')
            rig = (palette,binds,mapping)
        external_bones = {}
        if args.skeleton:
            external = parse_xml(args.skeleton)
            external_bones = skeleton(external.find('Drawable') if external.tag == 'Fragment' else external)
        textures = {}
        geometries = []
        jobs = [(drawable,None,None) for drawable in roots]
        if root.tag == 'Fragment' and args.type == 'vehicle':
            jobs += fragment_children(root,report)
        for drawable, child_matrix, inherited_shaders in jobs:
            bones = skeleton(drawable) or external_bones
            shaders = drawable.findall('ShaderGroup/Shaders/Item') or inherited_shaders or []
            models = next((drawable.findall(f'DrawableModels{lod}/Item') for lod in ('High','Medium','Low','VeryLow') if drawable.findall(f'DrawableModels{lod}/Item')),[])
            for model in models:
                for mesh in model.findall('Geometries/Item'):
                    buffer = mesh.find('VertexBuffer')
                    require(buffer is not None, 'Missing vertex buffer')
                    layout_node = buffer.find('Layout')
                    require(layout_node is not None, 'Missing vertex layout')
                    layout = [item.tag for item in layout_node]
                    require(all(field in SEMANTICS for field in layout) and 'Position' in layout, 'Unsupported vertex layout')
                    rows = (buffer.findtext('Data') or buffer.findtext('Data2') or '').strip().splitlines()
                    require(len(rows) <= 65535, 'Vertex budget exceeded')
                    source_palette = [int(v) for v in mesh.findtext('BoneIDs','').replace(',',' ').split()]
                    vertices = []
                    for row in rows:
                        columns = [float(v) for v in row.split()]
                        require(len(columns) == sum(SEMANTICS[f] for f in layout) and all(math.isfinite(v) for v in columns), 'Invalid vertex row')
                        fields = {}; offset = 0
                        for field in layout:
                            fields[field] = columns[offset:offset+SEMANTICS[field]]; offset += SEMANTICS[field]
                        vertex = {'pos':fields['Position'], 'normal':fields.get('Normal',[0.,0.,1.]),
                                  'uv':fields.get('TexCoord0',[0.,0.]), 'colour':[int(max(0,min(255,v))) for v in fields.get('Colour0',[255]*4)]}
                        if args.flip_v: vertex['uv'][1] = 1 - vertex['uv'][1]
                        if rig:
                            require('BlendWeights' in fields and 'BlendIndices' in fields and bones, 'Player/clothing conversion requires skin weights and an external or embedded skeleton')
                            palette, binds, mapping = rig
                            weights = fields['BlendWeights']; total = sum(weights)
                            require(total > 0 and all(w >= 0 for w in weights), 'Invalid clothing weights')
                            weights = [w / total for w in weights]
                            vertex['bones'] = []; vertex['weights'] = weights
                            posed = [0.,0.,0.]; normal = [0.,0.,0.]
                            for slot, weight in zip(fields['BlendIndices'], weights):
                                if weight == 0: vertex['bones'].append(0); continue
                                index = int(slot)
                                require(index == slot and 0 <= index < len(source_palette), 'Invalid clothing bone palette index')
                                bone = bones.get(source_palette[index]); require(bone is not None, 'Missing weighted source bone')
                                target = mapping.get(bone['name'],mapping.get(str(bone['tag'])))
                                require(target in palette, f"Missing bone mapping for {bone['name']} ({bone['tag']})")
                                target_index = palette.index(target); vertex['bones'].append(target_index)
                                matrix = mul(inverse(binds[target_index]),inverse(bone['world']))
                                for out, point in [(posed,transform(matrix,vertex['pos'])),(normal,normal_transform(matrix,vertex['normal']))]:
                                    for k in range(3): out[k] += point[k] * weight
                            vertex['pos'] = posed; vertex['normal'] = normal
                        elif child_matrix is not None:
                            vertex['pos'] = transform(child_matrix,vertex['pos'])
                            vertex['normal'] = normal_transform(child_matrix,vertex['normal'])
                        elif value(model,'HasSkin') == 0 and value(model,'BoneIndex') in bones:
                            matrix = bones[value(model,'BoneIndex')]['world']
                            vertex['pos'] = transform(matrix,vertex['pos']); vertex['normal'] = normal_transform(matrix,vertex['normal'])
                        length = math.sqrt(sum(v*v for v in vertex['normal']))
                        require(math.isfinite(length) and length > 1e-8, 'Invalid transformed normal')
                        vertex['normal'] = [v / length for v in vertex['normal']]
                        vertex['pos'] = [v * args.scale for v in vertex['pos']]
                        require(all(math.isfinite(v) and abs(v) < 10000 for v in vertex['pos']), 'Invalid transformed position')
                        vertices.append(vertex)
                    indices = [int(v) for v in mesh.findtext('IndexBuffer/Data','').split()]
                    shader_index = value(mesh,'ShaderIndex')
                    require(0 <= shader_index < len(shaders), 'Invalid shader index')
                    shader = shaders[shader_index]
                    diffuse = next((p.findtext('Name') for p in shader.findall('Parameters/Item') if p.get('name') == 'DiffuseSampler'),None)
                    texture = None; colour = (255,255,255)
                    if diffuse:
                        key = diffuse.lower()
                        if key in images:
                            if key not in textures:
                                name = 't' + hashlib.sha256(key.encode()).hexdigest()[:24]
                                textures[key] = (name,native_texture(name,read(images[key])))
                            texture = textures[key][0]
                        else:
                            colour = (45,45,45) if any(part in key for part in ('black','plastic','carbon','glass')) else (160,160,160)
                            report['warnings'].append(f'Missing shared/diffuse texture {diffuse}; using neutral colour {colour}')
                    alpha = 128 if value(shader,'RenderBucket') in (2,3) else 255
                    geometries.append(geometry(vertices,indices,texture,rig[:2] if rig else None,alpha,colour))
                    report['vertices'] += len(vertices); report['triangles'] += len(indices)//3
        payload = dff(geometries,frames)
        txd = chunk(22,chunk(1,S.pack('<HH',len(textures),2)) + b''.join(t[1] for t in textures.values()) + chunk(3,b'')) if textures else None
        require(len(payload) <= LIMIT and (txd is None or len(txd) <= LIMIT), 'Converted asset exceeds native 16 MiB file budget; reduce LOD/texture size')
        report.update(meshes=len(geometries),textures=len(textures),warnings=sorted(set(report['warnings'])))
        report['limitations'] = ['Highest available LOD only; GTA V shaders simplified to diffuse colour/alpha.',
                                 'No FiveM scripts, GTA V handling, damage, articulated wheels, cloth simulation or MLO portals.']
        manifest = {'schema_version':2,'enabled':args.enable,'name':args.out.name}
        entry = {'dff':'stream/converted.dff'}
        if txd: entry['txd'] = 'stream/converted.txd'
        if args.type == 'vehicle': manifest['vehicles'] = [entry]
        elif args.type == 'map':
            entry['id'] = args.model_id; manifest['models'] = [entry]
            manifest['placements'] = [{'model_id':args.model_id,'position':args.position}]
        elif args.type == 'player':
            manifest['player'] = dict(entry,ifp='stream/base.ifp')
        else:
            player = {'dff':'stream/base.dff','ifp':'stream/base.ifp','clothes':[dict(entry,name=args.out.name,enabled=True)]}
            if args.base_txd: player['txd'] = 'stream/base.txd'
            manifest['player'] = player
        args.out.parent.mkdir(parents=True,exist_ok=True)
        with tempfile.TemporaryDirectory(prefix='.sa-convert-',dir=args.out.parent) as staging:
            staging = Path(staging); (staging/'stream').mkdir(); (staging/'data').mkdir()
            (staging/'stream/converted.dff').write_bytes(payload)
            if txd: (staging/'stream/converted.txd').write_bytes(txd)
            if rig:
                base_files = [(args.base_ifp,'base.ifp')] if args.type == 'player' else [(args.base_player,'base.dff'),(args.base_ifp,'base.ifp'),(args.base_txd,'base.txd')]
                for path, name in base_files:
                    if path: (staging/'stream'/name).write_bytes(read(path))
            (staging/'resource.json').write_text(json.dumps(manifest,indent=2)+'\n',encoding='utf-8')
            (staging/'data/conversion-report.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
            require(not args.out.exists(), 'Output appeared during conversion; refusing overwrite')
            staging.rename(args.out)
        print(json.dumps(report,indent=2))
        print(f'Created {args.out}; enabled={args.enable}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('input',type=Path)
    parser.add_argument('--out',type=Path,required=True)
    parser.add_argument('--type',choices=['vehicle','player','clothing','map'],required=True)
    parser.add_argument('--textures',type=Path,help='YTD, YTD XML or directory containing DDS textures')
    parser.add_argument('--skeleton',type=Path,help='External CodeWalker YFT XML skeleton for player/clothing')
    parser.add_argument('--base-player',type=Path)
    parser.add_argument('--base-ifp',type=Path)
    parser.add_argument('--base-txd',type=Path)
    parser.add_argument('--bone-map',type=Path,help='JSON: GTA bone name/tag -> native HAnim bone ID')
    parser.add_argument('--scale',type=float,default=1.)
    parser.add_argument('--flip-v',action='store_true')
    parser.add_argument('--enable',action='store_true')
    parser.add_argument('--model-id',type=int,default=30000)
    parser.add_argument('--position',type=float,nargs=3,default=[2500.,-1670.,12.35])
    args = parser.parse_args()
    try:
        require(args.type not in ('clothing','player') or args.scale == 1, 'Skinned scale must be 1; fit the mesh to the base rig before conversion')
        require(all(math.isfinite(v) for v in args.position), 'Invalid placement position')
        require(math.isfinite(args.scale) and 0 < args.scale <= 100 and args.model_id >= 0,'Invalid scale or model ID')
        convert(args)
    except (ValueError,OSError,ET.ParseError,S.error,subprocess.CalledProcessError) as error:
        parser.exit(1,f'Conversion failed: {error}\n')


if __name__ == '__main__':
    main()
