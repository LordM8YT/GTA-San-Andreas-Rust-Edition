"""Translate static CodeWalker CMapData entities to native SA placements.

Both CEntityDef and the native IPL loader store inverse rotation quaternions.
MLO instances use different rules and are deliberately rejected here.
"""
import math
from pathlib import Path


def require(condition, message):
    if not condition:
        raise ValueError(message)


def jenkins(name):
    value = 0
    for byte in name.lower().encode('ascii'):
        value = (value + byte) & 0xffffffff
        value = (value + (value << 10)) & 0xffffffff
        value ^= value >> 6
    value = (value + (value << 3)) & 0xffffffff
    value ^= value >> 11
    return (value + (value << 15)) & 0xffffffff


def number(node, name):
    child = node.find(name)
    require(child is not None, f'Entity missing {name}')
    value = float(child.get('value', child.text or ''))
    require(math.isfinite(value), f'Invalid entity {name}')
    return value


def vector(node, name, axes):
    child = node.find(name)
    require(child is not None, f'Entity missing {name}')
    values = [float(child.get(axis, 'nan')) for axis in axes]
    require(all(math.isfinite(value) for value in values), f'Invalid entity {name}')
    return values


def placements(path, models, start_id, offset, parse_xml):
    require(path.name.lower().endswith('.ymap.xml'), 'Map import currently requires CodeWalker .ymap.xml')
    require(len(offset) == 3 and all(math.isfinite(v) and abs(v) < 10000 for v in offset), 'Invalid map offset')
    root = parse_xml(path)
    require(root.tag == 'CMapData', 'Expected CodeWalker CMapData XML')
    entities = root.find('entities')
    require(entities is not None, 'YMAP has no entities section')
    require(len(entities) <= 4096, 'YMAP exceeds 4096 entity budget')
    names = {}
    hashes = {}
    for index, model in enumerate(models):
        name = Path(model).name.lower()
        if name.endswith('.xml'):
            name = name[:-4]
        name = Path(name).stem
        require(name.isascii(), 'Map model names must be ASCII for GTA name hashes')
        require(name not in names, f'Ambiguous model basename: {name}')
        hash_ = jenkins(name)
        require(hash_ not in hashes, f'Ambiguous model hash: {name}')
        names[name] = start_id + index
        hashes[hash_] = start_id + index
    result = []
    skipped = []
    for index, item in enumerate(entities):
        require(item.tag == 'Item' and item.get('type') == 'CEntityDef',
                f'Unsupported YMAP entity {index}: MLO instances and non-CEntityDef types need separate support')
        lod = item.findtext('lodLevel') or ''
        if lod not in ('LODTYPES_DEPTH_HD', 'LODTYPES_DEPTH_ORPHANHD'):
            require(lod in ('LODTYPES_DEPTH_LOD', 'LODTYPES_DEPTH_SLOD1', 'LODTYPES_DEPTH_SLOD2', 'LODTYPES_DEPTH_SLOD3', 'LODTYPES_DEPTH_SLOD4'),
                    f'Unsupported entity LOD level: {lod}')
            skipped.append(dict(entity=index, reason=lod))
            continue
        archetype = (item.findtext('archetypeName') or '').strip().lower()
        model_id = names.get(archetype)
        if model_id is None:
            if archetype.startswith('hash_'):
                try:
                    model_id = hashes.get(int(archetype[5:], 16))
                except ValueError:
                    pass
            elif archetype.isdecimal():
                model_id = hashes.get(int(archetype))
        require(model_id is not None, f'Missing YDR model for archetype {archetype!r}; GTA V base-game props and YTYP aliases are not resolved')
        require(abs(number(item, 'scaleXY') - 1.) < 0.00001 and abs(number(item, 'scaleZ') - 1.) < 0.00001,
                f'Entity {index} has unsupported scale; bake scale into its model before import')
        position = [a + b for a, b in zip(vector(item, 'position', 'xyz'), offset)]
        require(all(abs(v) < 10000 for v in position), 'Translated placement is outside supported bounds')
        rotation = vector(item, 'rotation', 'xyzw')
        norm = sum(value * value for value in rotation)
        require(0.98 <= norm <= 1.02, 'Invalid entity rotation quaternion')
        rotation = [value / math.sqrt(norm) for value in rotation]
        result.append(dict(model_id=model_id, position=position, rotation=rotation))
    require(result, 'No supported HD static entities in YMAP')
    return result, skipped
