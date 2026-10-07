"""Bounded classic PC RenderWare rigid geometry decoder, implemented for this PoC."""
import math

from load_assets import Reader, chunks, one, root, cstring


def transform_point(matrix, point):
    return [sum(matrix[c*3+r]*point[c] for c in range(3))+matrix[9+r] for r in range(3)]


def compose(parent, child):
    axes = [sum(parent[k*3+r]*child[c*3+k] for k in range(3))
            for c in range(3) for r in range(3)]
    return axes + transform_point(parent,child[9:12])


def materials(data):
    reader = Reader(one(data,1))
    count = reader.unpack('<I')[0]
    if not 0 < count <= 256:
        raise ValueError('Material count outside bounds')
    references = [reader.unpack('<i')[0] for _ in range(count)]
    if reader.pos != len(reader.data):
        raise ValueError('Invalid material list size')
    fresh = iter(body for tag,body,_ in chunks(data) if tag == 7)
    result = []
    for i,ref in enumerate(references):
        if ref != -1:
            if not 0 <= ref < i:
                raise ValueError('Invalid material reuse index')
            result.append(result[ref])
            continue
        body = next(fresh,None)
        if body is None:
            raise ValueError('Missing material chunk')
        rd = Reader(one(body,1))
        rd.take(4)
        color = list(rd.unpack('<4B'))
        _,textured = rd.unpack('<II')
        if len(rd.data)-rd.pos not in (0,12):
            raise ValueError('Unsupported material structure')
        texture = None
        if textured:
            tex = one(body,6)
            names = [body for tag,body,_ in chunks(tex) if tag == 2]
            if len(names) != 2:
                raise ValueError('Invalid material texture names')
            texture = cstring(names[0]).lower()
        result.append(dict(color=color,texture=texture))
    if next(fresh,None) is not None:
        raise ValueError('Excess material chunks')
    return result


def geometry(data):
    for tag,body,_ in chunks(data):
        if tag == 3 and any(t == 0x116 for t,_,_ in chunks(body)):
            raise ValueError('Skinned geometry is not a static world model')
    rd = Reader(one(data,1))
    flags,nt,nv,nm = rd.unpack('<4I')
    if flags & 0x01000000 or nm != 1:
        raise ValueError('Native or multi-morph geometry unsupported')
    if not 0 < nv <= 65535 or not 0 < nt <= 200000:
        raise ValueError('Geometry counts outside bounds')
    nuv = (flags >> 16) & 255
    if not nuv:
        nuv = 2 if flags & 0x80 else int(bool(flags & 4))
    if nuv > 8:
        raise ValueError('Excess UV sets')
    colors = [list(rd.unpack('<4B')) for _ in range(nv)] if flags & 8 else [[255]*4 for _ in range(nv)]
    uvs = [[0,0] for _ in range(nv)]
    for layer in range(nuv):
        values = [list(rd.unpack('<2f')) for _ in range(nv)]
        if layer == 0:
            uvs = values
    tris = []
    for _ in range(nt):
        b,a,mat,c = rd.unpack('<4H')
        if max(a,b,c) >= nv:
            raise ValueError('Invalid triangle vertex index')
        tris.append([a,b,c,mat])
    sphere = rd.unpack('<4f')
    has_positions,has_normals = rd.unpack('<II')
    if not has_positions:
        raise ValueError('Missing geometry positions')
    vertices = [list(rd.unpack('<3f')) for _ in range(nv)]
    normals = [list(rd.unpack('<3f')) for _ in range(nv)] if has_normals else []
    if rd.pos != len(rd.data):
        raise ValueError('Unsupported geometry structure tail')
    mats = materials(one(data,8))
    if any(t[3] >= len(mats) for t in tris):
        raise ValueError('Invalid triangle material index')
    if not all(math.isfinite(v) for row in [*vertices,*normals,*uvs,sphere] for v in row):
        raise ValueError('Non-finite geometry values')
    return dict(vertices=vertices,normals=normals,uvs=uvs,colors=colors,triangles=tris,materials=mats)


def decode_static_dff(data):
    clump = root(data,16)
    sections = list(chunks(clump))
    if not sections or sections[0][0] != 1:
        raise ValueError('Missing clump structure')
    rd = Reader(sections[0][1])
    na,nl,nc = rd.unpack('<III')
    if not 0 < na <= 128 or not 0 <= nl <= 128 or nc:
        raise ValueError('Unsupported clump counts')
    # Attached RenderWare lights are metadata; this PoC supplies its own light.
    if sum(tag == 18 for tag,_,_ in sections) != nl:
        raise ValueError('Invalid attached light count')
    rd = Reader(one(one(clump,14),1))
    nf = rd.unpack('<I')[0]
    if not 0 < nf <= 256:
        raise ValueError('Frame count outside bounds')
    local,parents = [],[]
    for i in range(nf):
        matrix = list(rd.unpack('<12f'))
        parent,_ = rd.unpack('<iI')
        if not -1 <= parent < nf or parent == i or not all(math.isfinite(v) for v in matrix):
            raise ValueError('Invalid frame hierarchy')
        local.append(matrix);parents.append(parent)
    if rd.pos != len(rd.data):
        raise ValueError('Invalid frame list size')
    resolved,visiting = {},set()
    def frame(i):
        if i in visiting:
            raise ValueError('Cyclic frame hierarchy')
        if i not in resolved:
            visiting.add(i)
            resolved[i] = local[i] if parents[i] == -1 else compose(frame(parents[i]),local[i])
            visiting.remove(i)
        return resolved[i]
    for i in range(nf):
        frame(i)
    gl = one(clump,26)
    ng = Reader(one(gl,1)).unpack('<I')[0]
    gs = [geometry(body) for tag,body,_ in chunks(gl) if tag == 15]
    if not 0 < ng <= 128 or ng != len(gs):
        raise ValueError('Invalid geometry list count')
    atomics = [body for tag,body,_ in chunks(clump) if tag == 20]
    if len(atomics) != na:
        raise ValueError('Invalid atomic count')
    result = []
    for body in atomics:
        fi,gi,_,_ = Reader(one(body,1)).unpack('<4I')
        if fi >= nf or gi >= ng:
            raise ValueError('Invalid atomic frame/geometry reference')
        g = gs[gi]
        matrix = frame(fi)
        result.append({**g,'vertices':[transform_point(matrix,v) for v in g['vertices']],
                       'normals':[[sum(matrix[c*3+r]*v[c] for c in range(3)) for r in range(3)] for v in g['normals']]})
    return result
