"""Small bounded RenderWare decoder for the verified first static model.

Scope: SA 3.6 non-native geometry, one frame/atomic/geometry/material, D3D9 DXT1.
Unsupported layouts fail explicitly rather than silently drawing wrong meshes.
"""
import json
import math
from pathlib import Path
import struct
import zlib


class Reader:
    def __init__(self, data):
        self.data = memoryview(data)
        self.pos = 0

    def take(self, count):
        if count < 0 or self.pos + count > len(self.data):
            raise ValueError('Truncated RenderWare data')
        result = self.data[self.pos:self.pos+count]
        self.pos += count
        return result

    def unpack(self, fmt):
        return struct.unpack(fmt, self.take(struct.calcsize(fmt)))


def chunks(data):
    reader = Reader(data)
    while reader.pos < len(reader.data):
        kind, size, version = reader.unpack('<III')
        yield kind, reader.take(size), version


def one(data, kind):
    matches = [body for tag, body, _ in chunks(data) if tag == kind]
    if len(matches) != 1:
        raise ValueError(f'Expected exactly one chunk {kind:#x}, got {len(matches)}')
    return matches[0]


def root(data, expected):
    reader = Reader(data)
    kind, size, version = reader.unpack('<III')
    if kind != expected or version != 0x1803FFFF:
        raise ValueError(f'Unsupported root/version: {kind:#x}/{version:#x}')
    return reader.take(size)  # IMG sector padding is intentionally ignored.


def cstring(data):
    return bytes(data).split(b'\0', 1)[0].decode('ascii', errors='strict')


def decode_dff(data):
    clump = root(data, 0x10)
    clump_struct = Reader(one(clump, 1))
    atomics, lights, cameras = clump_struct.unpack('<III')
    if (atomics, lights, cameras) != (1, 0, 0):
        raise ValueError('PoC supports one static atomic, no lights/cameras')
    frame = Reader(one(one(clump, 0xE), 1))
    if frame.unpack('<I')[0] != 1:
        raise ValueError('PoC supports exactly one frame')
    transform = frame.unpack('<12f')
    parent, flags = frame.unpack('<iI')
    if parent != -1:
        raise ValueError('Unsupported frame parent')
    geometry_list = one(clump, 0x1A)
    if Reader(one(geometry_list, 1)).unpack('<I')[0] != 1:
        raise ValueError('PoC supports exactly one geometry')
    atomic = Reader(one(one(clump, 0x14), 1))
    frame_index, geometry_index, _, _ = atomic.unpack('<IIII')
    if frame_index or geometry_index:
        raise ValueError('Unexpected atomic frame/geometry reference')
    geometry = one(geometry_list, 0xF)
    reader = Reader(one(geometry, 1))
    flags, triangles_count, vertices_count, morphs = reader.unpack('<IIII')
    if flags & 0x01000000 or morphs != 1:
        raise ValueError('Native geometry or multiple morph targets unsupported')
    if not 0 < vertices_count <= 65535 or not 0 < triangles_count <= 200000:
        raise ValueError('Geometry count outside PoC bounds')
    uv_sets = (flags >> 16) & 255
    if uv_sets == 0:
        uv_sets = 2 if flags & 0x80 else (1 if flags & 4 else 0)
    if uv_sets != 1:
        raise ValueError('PoC requires one UV set')
    colors = [list(reader.unpack('<4B')) for _ in range(vertices_count)] if flags & 8 else []
    uvs = [list(reader.unpack('<2f')) for _ in range(vertices_count)]
    triangles = []
    for _ in range(triangles_count):
        b, a, material, c = reader.unpack('<4H')
        if max(a, b, c) >= vertices_count or material != 0:
            raise ValueError('Invalid vertex/material index')
        triangles.append([a, b, c])
    sphere = reader.unpack('<4f')
    has_vertices, has_normals = reader.unpack('<II')
    if not has_vertices:
        raise ValueError('Geometry has no positions')
    positions = [reader.unpack('<3f') for _ in range(vertices_count)]
    if has_normals:
        reader.take(vertices_count * 12)
    if reader.pos != len(reader.data):
        raise ValueError('Unexpected geometry struct tail')
    if not all(math.isfinite(x) for row in [*positions, *uvs, transform, sphere] for x in row):
        raise ValueError('Non-finite geometry values')
    # Frame matrix columns are right/up/at/translation. Convert GTA Z-up to Y-up.
    vertices = []
    for x, y, z in positions:
        gx = transform[0]*x + transform[3]*y + transform[6]*z + transform[9]
        gy = transform[1]*x + transform[4]*y + transform[7]*z + transform[10]
        gz = transform[2]*x + transform[5]*y + transform[8]*z + transform[11]
        vertices.append([gx, gz, -gy])
    materials = one(geometry, 8)
    material_list = Reader(one(materials, 1))
    if material_list.unpack('<I')[0] != 1 or material_list.unpack('<i')[0] != -1:
        raise ValueError('PoC supports one fresh material')
    material = one(materials, 7)
    material_struct = Reader(one(material, 1))
    material_struct.take(4)
    material_color = list(material_struct.unpack('<4B'))
    _, textured = material_struct.unpack('<II')
    if not textured:
        raise ValueError('First model requires textured material')
    texture = one(material, 6)
    strings = [body for kind, body, _ in chunks(texture) if kind == 2]
    if len(strings) != 2:
        raise ValueError('Invalid texture name/mask strings')
    return {'name': 'CJ_WASTEBIN', 'vertices': vertices, 'uvs': uvs,
            'triangles': triangles, 'prelight': colors, 'material_color': material_color,
            'texture': cstring(strings[0]), 'vertex_count': vertices_count,
            'triangle_count': triangles_count}


def rgb565(value):
    return [(value >> 11) * 255 // 31, ((value >> 5) & 63) * 255 // 63,
            (value & 31) * 255 // 31, 255]


def dxt1(data, width, height):
    if not 0 < width <= 4096 or not 0 < height <= 4096:
        raise ValueError('Invalid DXT1 dimensions')
    expected = ((width + 3)//4) * ((height + 3)//4) * 8
    if len(data) != expected:
        raise ValueError('Invalid DXT1 payload length')
    out = bytearray(width * height * 4)
    reader = Reader(data)
    for by in range(0, height, 4):
        for bx in range(0, width, 4):
            c0, c1, bits = reader.unpack('<HHI')
            palette = [rgb565(c0), rgb565(c1)]
            if c0 > c1:
                palette += [[(2*palette[0][k]+palette[1][k])//3 for k in range(3)]+[255],
                            [(palette[0][k]+2*palette[1][k])//3 for k in range(3)]+[255]]
            else:
                palette += [[(palette[0][k]+palette[1][k])//2 for k in range(3)]+[255], [0,0,0,0]]
            for p in range(16):
                x, y = bx + p%4, by + p//4
                if x < width and y < height:
                    offset = (y*width+x)*4
                    out[offset:offset+4] = bytes(palette[(bits >> (2*p)) & 3])
    return bytes(out)


def decode_txd(data):
    dictionary = root(data, 0x16)
    count, device = Reader(one(dictionary, 1)).unpack('<HH')
    native = [body for kind, body, _ in chunks(dictionary) if kind == 0x15]
    if len(native) != count or not 0 < count <= 256:
        raise ValueError('Invalid texture dictionary count')
    result = {}
    for body in native:
        reader = Reader(one(body, 1))
        platform, filtering = reader.unpack('<II')
        name, mask = cstring(reader.take(32)), cstring(reader.take(32))
        raster, fourcc = reader.unpack('<I4s')
        width, height, depth, levels, raster_type, properties = reader.unpack('<HHBBBB')
        if platform != 9 or fourcc != b'DXT1' or raster & 0x6000 or properties & 2:
            raise ValueError(f'Unsupported native texture: {name}')
        if not 1 <= levels <= 13:
            raise ValueError('Invalid texture mip count')
        rgba = None
        for level in range(levels):
            size = reader.unpack('<I')[0]
            pixels = reader.take(size)
            expected = ((max(1,width>>level)+3)//4)*((max(1,height>>level)+3)//4)*8
            if size != expected:
                raise ValueError('Invalid DXT1 mip payload')
            if level == 0:
                rgba = dxt1(pixels, width, height)
        if reader.pos != len(reader.data) or name.lower() in result:
            raise ValueError('Invalid texture tail or duplicate name')
        result[name.lower()] = {'name': name, 'width': width, 'height': height,
                                'rgba': rgba, 'format': 'D3D9 DXT1', 'levels': levels}
    return result


def png(texture):
    width, height, rgba = texture['width'], texture['height'], texture['rgba']
    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind+data))
    rows = b''.join(b'\0' + rgba[y*width*4:(y+1)*width*4] for y in range(height))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', width,height,8,6,0,0,0))
            + chunk(b'IDAT', zlib.compress(rows)) + chunk(b'IEND', b''))


def load_pair(dff, txd):
    model = decode_dff(dff)
    textures = decode_txd(txd)
    key = model['texture'].lower()
    if key not in textures:
        raise ValueError(f'Material texture not found: {model["texture"]}')
    texture = textures[key]
    model['texture_info'] = {k: v for k, v in texture.items() if k != 'rgba'}
    model['dictionary_textures'] = list(textures)
    return model, texture
