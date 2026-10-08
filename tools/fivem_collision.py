"""Bake a bounded subset of CodeWalker YBN into one native COL1 model.

Coordinates remain Z-up. Geometry vertices include GeometryCenter before
child CompositeTransform matrices are composed. GTA material/flag semantics
are not native SA surfaces and are deliberately not copied.
"""
import math
import struct


def require(condition, message):
    if not condition:
        raise ValueError(message)


IDENTITY = [1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1.]


def vector(node, name):
    child = node.find(name)
    require(child is not None, f'Collision missing {name}')
    values = [float(child.get(axis, 'nan')) for axis in 'xyz']
    require(all(math.isfinite(v) for v in values), f'Invalid collision {name}')
    return values


def matrix(node):
    child = node.find('CompositeTransform')
    if child is None:
        return IDENTITY
    values = [float(v) for v in (child.text or '').replace(',', ' ').split()]
    require(len(values) == 16 and all(math.isfinite(v) for v in values), 'Invalid collision transform')
    require(all(abs(values[i]) < 1e-7 for i in (3, 7, 11)) and abs(values[15] - 1) < 1e-7,
            'Collision transform must be affine')
    determinant = (values[0]*(values[5]*values[10]-values[9]*values[6])
                   - values[4]*(values[1]*values[10]-values[9]*values[2])
                   + values[8]*(values[1]*values[6]-values[5]*values[2]))
    require(abs(determinant) > 1e-8, 'Singular collision transform')
    return values


def triangles(path, parse_xml):
    root = parse_xml(path)
    require(root.tag == 'BoundsFile', 'Expected CodeWalker BoundsFile YBN XML')
    bounds = root.find('Bounds')
    require(bounds is not None, 'YBN has no Bounds')
    result, nodes = [], 0

    def visit(node, parent, depth):
        nonlocal nodes
        nodes += 1
        require(depth <= 16 and nodes <= 4096, 'YBN composite nesting/node budget exceeded')
        child = matrix(node)
        transform = [sum(parent[k*4+row]*child[col*4+k] for k in range(4))
                     for col in range(4) for row in range(4)]

        def emit(points):
            face = [tuple(sum(transform[col*4+row]*point[col] for col in range(3))
                          + transform[12+row] for row in range(3)) for point in points]
            require(all(math.isfinite(v) and abs(v) < 10000 for p in face for v in p),
                    'Collision point exceeds native coordinate budget')
            # Degenerate triangles cannot provide reliable road/wall contacts.
            a = [face[1][i]-face[0][i] for i in range(3)]
            b = [face[2][i]-face[0][i] for i in range(3)]
            cross = [a[1]*b[2]-a[2]*b[1], a[2]*b[0]-a[0]*b[2], a[0]*b[1]-a[1]*b[0]]
            require(sum(v*v for v in cross) > 1e-14, 'Degenerate YBN triangle')
            result.append(face)
            require(len(result) <= 65535, 'YBN exceeds native 65535 triangle budget')

        kind = node.get('type')
        if kind == 'Composite':
            children = node.find('Children')
            require(children is not None and len(children) > 0, 'Empty YBN composite')
            for item in children:
                require(item.tag == 'Item', 'Invalid composite child')
                visit(item, transform, depth+1)
        elif kind in ('Geometry', 'GeometryBVH'):
            center = vector(node, 'GeometryCenter')
            rows = (node.findtext('Vertices') or '').strip().splitlines()
            require(0 < len(rows) <= 65536, 'YBN vertex budget exceeded or empty')
            vertices = []
            for row in rows:
                values = [float(v) for v in row.replace(',', ' ').split()]
                require(len(values) == 3 and all(math.isfinite(v) for v in values), 'Invalid YBN vertex')
                vertices.append([v+c for v, c in zip(values, center)])
            polygons = node.find('Polygons')
            require(polygons is not None and 0 < len(polygons) <= 65535, 'YBN polygon budget exceeded or empty')
            for polygon in polygons:
                require(polygon.tag == 'Triangle', f'Unsupported YBN polygon {polygon.tag}; triangle meshes required')
                indices = [int(polygon.get(axis, '-1')) for axis in ('v1', 'v2', 'v3')]
                require(all(0 <= index < len(vertices) for index in indices), 'YBN triangle vertex index out of bounds')
                emit([vertices[index] for index in indices])
        elif kind == 'Box':
            low, high = vector(node, 'BoxMin'), vector(node, 'BoxMax')
            require(all(a < b for a, b in zip(low, high)), 'Invalid YBN box bounds')
            vertices = [[high[i] if n & (1 << i) else low[i] for i in range(3)] for n in range(8)]
            for a, b, c, d in ((0,1,3,2), (4,6,7,5), (0,4,5,1), (2,3,7,6), (0,2,6,4), (1,5,7,3)):
                emit([vertices[a], vertices[b], vertices[c]])
                emit([vertices[a], vertices[c], vertices[d]])
        else:
            raise ValueError(f'Unsupported YBN bound {kind}; supported: Composite, Geometry/GeometryBVH triangles, Box')

    visit(bounds, IDENTITY, 0)
    require(result, 'YBN has no supported collision triangles')
    return result


def col(faces):
    vertices, indices, lookup = [], [], {}
    for face in faces:
        triangle = []
        for point in face:
            if point not in lookup:
                lookup[point] = len(vertices)
                vertices.append(point)
                require(len(vertices) <= 65536, 'Native COL vertex budget exceeded')
            triangle.append(lookup[point])
        indices.append(triangle)
    low = [min(p[i] for p in vertices) for i in range(3)]
    high = [max(p[i] for p in vertices) for i in range(3)]
    center = [(a+b)/2 for a, b in zip(low, high)]
    radius = math.sqrt(sum(((b-a)/2)**2 for a, b in zip(low, high)))
    body = b'fivem_collision'.ljust(22, b'\0') + bytes(2)
    body += struct.pack('<10f', radius, *center, *low, *high)
    body += struct.pack('<4I', 0, 0, 0, len(vertices))
    body += b''.join(struct.pack('<3f', *point) for point in vertices)
    body += struct.pack('<I', len(indices))
    body += b''.join(struct.pack('<3I4B', *face, 0, 0, 0, 255) for face in indices)
    require(len(body)+8 <= 16*1024*1024, 'COL exceeds native file budget')
    return b'COLL' + struct.pack('<I', len(body)) + body, low, high


def invisible_dff(low, high, converter):
    # Bounds vertices keep region selection sane; the zero-area transparent
    # triangle contributes no visible pixels. Explicit COL supplies contacts.
    vertices = [dict(pos=p, normal=[0., 0., 1.], uv=[0., 0.], colour=[255,255,255,0]) for p in (low, high)]
    return converter.dff([converter.geometry(vertices, [0,0,0], None, alpha=0)])
