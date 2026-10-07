"""Translate static CodeWalker CMapData entities to native SA placements.

Both CEntityDef and the native IPL loader store inverse rotation quaternions.
MLO instances use different rules and are deliberately rejected here.
"""
import math
from pathlib import Path
import re


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


def hash_reference(name):
    name = name.strip().lower()
    if re.fullmatch(r'hash_[0-9a-f]{8}', name):
        return int(name[5:], 16)
    if name.isdecimal():
        value = int(name)
        require(value <= 0xffffffff, 'Name hash exceeds 32 bits')
        return value
    require(re.fullmatch(r'[a-z0-9_.-]{1,128}', name), f'Invalid archetype/asset name: {name!r}')
    return jenkins(name)


def model_index(models, start_id):
    names, hashes = {}, {}
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
    return names, hashes


def static_archetypes(paths, models, start_id, parse_xml):
    """Resolve explicit static YTYP aliases; no MLO/time/extension behavior."""
    names, hashes = model_index(models, start_id)
    aliases, textures, total = {}, {}, 0
    for path in paths:
        root = parse_xml(path)
        require(root.tag == 'CMapTypes', 'Expected CodeWalker CMapTypes YTYP XML')
        for section in ('extensions', 'compositeEntityTypes'):
            node = root.find(section)
            require(node is None or (len(node) == 0 and not (node.text or '').strip()), f'Unsupported YTYP {section}')
        entries = root.find('archetypes')
        require(entries is not None, 'YTYP has no archetypes section')
        total += len(entries)
        require(total <= 4096, 'YTYP exceeds 4096 archetype budget')
        for item in entries:
            require(item.tag == 'Item' and item.get('type') == 'CBaseArchetypeDef',
                    'Only static CBaseArchetypeDef YTYP entries supported; MLO/time archetypes require separate support')
            require(item.findtext('assetType') == 'ASSET_TYPE_DRAWABLE', 'YTYP requires an individual DRAWABLE asset')
            extensions = item.find('extensions')
            require(extensions is None or (len(extensions) == 0 and not (extensions.text or '').strip()), 'Unsupported YTYP archetype extensions')
            asset = (item.findtext('assetName') or '').strip().lower()
            model_id = names.get(asset)
            if model_id is None:
                model_id = hashes.get(hash_reference(asset))
            require(model_id is not None, f'Missing selected YDR for YTYP asset {asset!r}')
            alias = hash_reference(item.findtext('name') or '')
            require(alias not in aliases, 'Duplicate YTYP archetype name/hash')
            require(alias not in hashes or hashes[alias] == model_id, 'YTYP alias conflicts with a model basename/hash')
            aliases[alias] = model_id
            dictionary = (item.findtext('textureDictionary') or '').strip().lower()
            if dictionary and hash_reference(dictionary) != 0:
                dictionary = hash_reference(dictionary)
                require(model_id not in textures or textures[model_id] == dictionary,
                        'Conflicting YTYP texture dictionaries for one YDR')
                textures[model_id] = dictionary
    return aliases, textures


def placements(path, models, start_id, offset, parse_xml, aliases=None):
    require(path.name.lower().endswith('.ymap.xml'), 'Map import currently requires CodeWalker .ymap.xml')
    require(len(offset) == 3 and all(math.isfinite(v) and abs(v) < 10000 for v in offset), 'Invalid map offset')
    root = parse_xml(path)
    require(root.tag == 'CMapData', 'Expected CodeWalker CMapData XML')
    entities = root.find('entities')
    require(entities is not None, 'YMAP has no entities section')
    require(len(entities) <= 4096, 'YMAP exceeds 4096 entity budget')
    names, hashes = model_index(models, start_id)
    hashes.update(aliases or {})
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
            model_id = hashes.get(hash_reference(archetype))
        require(model_id is not None, f'Missing YDR model for archetype {archetype!r}; include custom assets and --ytyp for aliases; GTA V base-game props are not resolved')
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
