#!/usr/bin/env python3
"""Inspect a FiveM resource without executing Lua; convert models into native resources."""
import argparse
import contextlib
import hashlib
import importlib.util
import io
import json
import math
import os
from pathlib import Path
import re
import shutil
import stat
import tempfile
from types import SimpleNamespace

spec = importlib.util.spec_from_file_location('convert_gta5', Path(__file__).with_name('convert-gta5.py'))
converter = importlib.util.module_from_spec(spec)
spec.loader.exec_module(converter)
map_spec = importlib.util.spec_from_file_location('fivem_map', Path(__file__).with_name('fivem_map.py'))
map_converter = importlib.util.module_from_spec(map_spec)
map_spec.loader.exec_module(map_converter)
collision_spec = importlib.util.spec_from_file_location('fivem_collision', Path(__file__).with_name('fivem_collision.py'))
collision_converter = importlib.util.module_from_spec(collision_spec)
collision_spec.loader.exec_module(collision_converter)
MAX_BYTES = 512 * 1024 * 1024
MAX_FILES = 4096
ASSET_EXTENSIONS = ('.yft', '.ydr', '.ydd', '.ytd', '.ymap', '.ytyp', '.ybn')


def require(condition, message):
    if not condition:
        raise ValueError(message)


def checked_tree(root):
    """Reject links/junctions before traversal, including links inside the root."""
    require(root.is_dir(), 'Input must be an extracted resource directory')
    files = []
    total = 0
    entries_seen = 0

    def visit(directory, depth):
        nonlocal total, entries_seen
        require(depth <= 12, 'Resource directory nesting exceeds 12 levels')
        with os.scandir(directory) as entries:
            children = sorted(entries, key=lambda entry: entry.name.casefold())
        for entry in children:
            entries_seen += 1
            require(entries_seen <= MAX_FILES, 'Resource has too many entries')
            info = entry.stat(follow_symlinks=False)
            require(not stat.S_ISLNK(info.st_mode) and not getattr(info, 'st_file_attributes', 0) & 0x400,
                    f'Links/junctions are unsupported: {entry.name}')
            path = Path(entry.path)
            require(path.resolve().is_relative_to(root.resolve()), 'Resource escapes input directory')
            if stat.S_ISDIR(info.st_mode):
                visit(path, depth + 1)
            else:
                require(stat.S_ISREG(info.st_mode), f'Not a regular file: {entry.name}')
                total += info.st_size
                require(info.st_size <= 128 * 1024 * 1024 and total <= MAX_BYTES,
                        'Resource exceeds 128 MiB per input file or 512 MiB total')
                files.append(path)

    info = root.lstat()
    require(not root.is_symlink() and not getattr(info, 'st_file_attributes', 0) & 0x400,
            'Input root must not be a link/junction')
    visit(root, 0)
    return files


def asset_name(path):
    name = path.name.lower()
    if name.endswith('.xml'):
        name = name[:-4]
    suffix = Path(name).suffix
    return name[:-len(suffix)], suffix


def inspect_resource(root):
    root = root.absolute()
    files = checked_tree(root)
    relative = lambda p: p.relative_to(root).as_posix()
    by_name = {}
    for path in files:
        key = relative(path).casefold()
        require(key not in by_name, 'Case-insensitive duplicate paths in resource')
        by_name[key] = path
    manifest = next((by_name[n] for n in ('fxmanifest.lua', '__resource.lua') if n in by_name), None)
    require(manifest is not None, 'No fxmanifest.lua or __resource.lua at resource root')
    require(manifest.stat().st_size <= 128 * 1024, 'FiveM manifest exceeds 128 KiB')
    # This is deliberately an inventory, not a Lua evaluator. Dynamic metadata
    # cannot be interpreted; scripts and manifests are never executed or copied.
    text = manifest.read_text(encoding='utf-8-sig')
    assets = [p for p in files if asset_name(p)[1] in ASSET_EXTENSIONS]
    scripts = [relative(p) for p in files if p != manifest and p.suffix.lower() in ('.lua', '.js', '.dll', '.cs')]
    notices = ['Manifest is inspected as text only; dynamic Lua declarations are not evaluated.']
    if scripts or re.search(r'\b(client_scripts?|server_scripts?|shared_scripts?)\b', text):
        notices.append('Client/server scripts and GTA V/Cfx natives are not imported.')
    if re.search(r'\b(ui_page|loadscreen)\b', text):
        notices.append('NUI/HUD web pages and loading screens are not imported.')
    if any(asset_name(p)[1] in ('.ymap', '.ytyp', '.ybn') for p in assets):
        notices.append('YMAP/YTYP require map conversion; YBN requires explicit --collision or --world-collision selection. MLO rooms/portals remain unsupported.')
    if any(asset_name(p)[1] == '.ydd' for p in assets):
        notices.append('YDD peds/clothes require --kind player/clothing and explicit native target rig/bone mapping.')
    metadata = [relative(p) for p in files if p.suffix.lower() == '.meta']
    if metadata:
        notices.append('GTA V handling, vehicle metadata, tuning and colours are not imported.')
    return files, dict(resource=root.name, manifest=relative(manifest),
                       assets=[relative(p) for p in assets], scripts=scripts,
                       metadata=metadata, warnings=notices)


def select_models(root, files, kind, requested=()):
    extension = '.yft' if kind == 'vehicles' else '.ydd' if kind in ('player', 'clothing') else '.ydr'
    models = [p for p in files if asset_name(p)[1] == extension]
    # Prefer raw resources when an XML export of the same asset sits beside it.
    models = [p for p in models if not (p.name.lower().endswith('.xml') and
              any(q.parent == p.parent and q.name.casefold() == p.name[:-4].casefold() for q in models))]
    if requested:
        requested = set(requested)
        chosen = [p for p in models if p.relative_to(root).as_posix() in requested]
        require(len(chosen) == len(requested), 'Requested model missing or not supported for this kind')
    else:
        # A car's base fragment retains children/wheels. Do not spawn its _hi
        # fragment as a second car; allow explicit selection for author review.
        chosen = [p for p in models if not (kind == 'vehicles' and asset_name(p)[0].endswith('_hi') and
                  any(q.parent == p.parent and asset_name(q)[0] == asset_name(p)[0][:-3] for q in models))]
    limit = {'vehicles': 32, 'player': 1, 'clothing': 16}.get(kind, 64)
    require(0 < len(chosen) <= limit,
            f'No compatible models, or native {kind} model budget exceeded ({limit}); select with --model')
    return sorted(chosen, key=lambda p: p.relative_to(root).as_posix().casefold())


def import_resource(root, output, kind, requested=(), position=None, model_id=30000, enable=False,
                    ymap=None, offset=(0., 0., 0.), ytyp=(), base_player=None, base_ifp=None,
                    base_txd=None, bone_map=None, skeleton=None, texture_map=(), collision_map=(), world_collision=()):
    root = root.absolute()
    output = output.absolute()
    require(not output.exists(), 'Output already exists; choose a new directory')
    require(not output.resolve().is_relative_to(root.resolve()), 'Output must be outside the source resource')
    require(kind != 'props' or position is not None, 'Props require --position X Y Z for their preview placements')
    require(kind != 'map' or ymap is not None, 'Map import requires --ymap relative/path.ymap or .ymap.xml')
    require(position is None or all(math.isfinite(v) and abs(v) < 10000 for v in position), 'Invalid position')
    require(0 <= model_id <= 2147483583, 'Invalid starting model ID')
    files, report = inspect_resource(root)
    if ymap is not None:
        require(kind == 'map', '--ymap requires --kind map')
        require(any(p.relative_to(root).as_posix() == ymap for p in files), 'YMAP must be an exact relative path inside the resource')
    require(not ytyp or kind == 'map', '--ytyp requires --kind map')
    require(len(ytyp) <= 16 and len(set(ytyp)) == len(ytyp), 'Select at most 16 distinct YTYP files')
    for path in ytyp:
        require(any(p.relative_to(root).as_posix() == path and asset_name(p)[1] == '.ytyp' for p in files),
                'YTYP must be an exact relative .ytyp or .ytyp.xml path inside the resource')
    chosen = select_models(root, files, kind, requested)
    skinned = kind in ('player', 'clothing')
    require(not skinned or (base_player and base_ifp and bone_map),
            'Player/clothing import needs --base-player native.dff, --base-ifp native.ifp and --bone-map map.json')
    require(skinned or not any((base_player, base_ifp, base_txd, bone_map, skeleton)),
            'Target rig and skeleton options require --kind player or clothing')
    require(not base_txd or kind == 'clothing', '--base-txd applies to the native base player in clothing imports only')
    if skeleton:
        require(any(p.relative_to(root).as_posix() == skeleton and p.name.lower().endswith('.xml')
                    and asset_name(p)[1] in ('.yft', '.ydr', '.ydd') for p in files),
                '--skeleton must be an exact relative CodeWalker model XML path inside the resource')
    texture_choices = {}
    for item in texture_map:
        model, separator, texture = item.partition('=')
        require(separator and model not in texture_choices, '--texture requires distinct MODEL=YTD pairs')
        require(any(p.relative_to(root).as_posix() == model for p in chosen), '--texture model must be selected')
        require(any(p.relative_to(root).as_posix() == texture and asset_name(p)[1] == '.ytd' for p in files),
                '--texture must reference an exact relative YTD or YTD XML path inside the resource')
        texture_choices[model] = texture
    require(not (collision_map or world_collision) or kind in ('props', 'map'),
            'YBN collision import requires --kind props or map')
    require(not world_collision or kind == 'map', '--world-collision requires --kind map')
    require(len(world_collision) <= 16 and len(set(world_collision)) == len(world_collision),
            'Select at most 16 distinct world collision files')
    require(model_id + len(chosen) + len(world_collision) - 1 <= 2147483647, 'Collision model IDs exceed native range')
    collision_choices = {}
    def collision_path(path):
        require(any(p.relative_to(root).as_posix() == path and asset_name(p)[1] == '.ybn' for p in files),
                'Collision must reference an exact relative YBN or YBN XML path inside the resource')
    for item in collision_map:
        model, separator, collision = item.partition('=')
        require(separator and model not in collision_choices, '--collision requires distinct MODEL=YBN pairs')
        require(any(p.relative_to(root).as_posix() == model for p in chosen), '--collision model must be selected')
        collision_path(collision)
        collision_choices[model] = collision
    for path in world_collision:
        collision_path(path)
        require(path not in collision_choices.values(), 'One YBN cannot be both model-local and world collision')
    output.parent.mkdir(parents=True, exist_ok=True)
    # Snapshot only data into an isolated tree. XML texture discovery cannot
    # wander through the source package or follow a changed source link.
    with tempfile.TemporaryDirectory(prefix='.sa-fivem-', dir=output.parent) as work:
        work = Path(work)
        source = work / 'source'; source.mkdir()
        fingerprints = {}
        snapshot_bytes = 0
        for path in files:
            if asset_name(path)[1] in ('.yft', '.ydr', '.ydd', '.ytd', '.ymap', '.ytyp', '.ybn') or path.suffix.lower() == '.dds':
                require(not path.is_symlink() and path.resolve().is_relative_to(root.resolve()), 'Source changed during import')
                data = converter.read(path, 128 * 1024 * 1024)
                snapshot_bytes += len(data)
                require(snapshot_bytes <= MAX_BYTES, 'Source changed beyond the 512 MiB input budget')
                destination = source / path.relative_to(root)
                destination.parent.mkdir(parents=True, exist_ok=True)
                destination.write_bytes(data)
                fingerprints[path.relative_to(root).as_posix()] = hashlib.sha256(data).hexdigest()
        # Repeat tree validation to catch newly introduced junctions.
        checked_tree(root)
        package = work / 'package'; package.mkdir()
        manifest = dict(schema_version=2, enabled=enable, name=root.name)
        if not skinned:
            manifest['vehicles' if kind == 'vehicles' else 'models'] = []
            if kind != 'vehicles': manifest['placements'] = []
        rig = {}
        if skinned:
            target = work / 'rig'; target.mkdir()
            report['target_rig'] = {}
            for key, path, filename in [('base_player', base_player, 'base.dff'),
                                        ('base_ifp', base_ifp, 'base.ifp'),
                                        ('base_txd', base_txd, 'base.txd'),
                                        ('bone_map', bone_map, 'bones.json')]:
                if path:
                    data = converter.read(Path(path), 128 * 1024 if key == 'bone_map' else 16 * 1024 * 1024)
                    rig[key] = target / filename; rig[key].write_bytes(data)
                    report['target_rig'][key] = dict(name=Path(path).name, sha256=hashlib.sha256(data).hexdigest())
            rig_folder = package / 'stream/rig'; rig_folder.mkdir(parents=True)
            for key in ('base_ifp', 'base_player', 'base_txd'):
                if key in rig and (key == 'base_ifp' or kind == 'clothing'):
                    shutil.copyfile(rig[key], rig_folder / rig[key].name)
            if kind == 'clothing':
                manifest['player'] = dict(dff='stream/rig/base.dff', ifp='stream/rig/base.ifp', clothes=[])
                if base_txd: manifest['player']['txd'] = 'stream/rig/base.txd'
        report['converted'] = []
        report['collisions'] = []
        report['source_sha256'] = fingerprints
        aliases, dictionaries = {}, {}
        if ytyp:
            type_sources = [converter.extract(source / path, work / f'ytyp-{index:03}') for index, path in enumerate(ytyp)]
            aliases, dictionaries = map_converter.static_archetypes(
                type_sources, [p.relative_to(root) for p in chosen], model_id, converter.parse_xml)
        for index, original in enumerate(chosen):
            model = source / original.relative_to(root)
            stem = asset_name(model)[0]
            texture_stem = stem[:-3] if stem.endswith('_hi') else stem
            textures = [p for p in source.rglob('*') if p.is_file() and
                        asset_name(p)[1] == '.ytd' and
                        (asset_name(p)[0].isascii() and map_converter.jenkins(asset_name(p)[0]) == dictionaries[model_id + index]
                         if model_id + index in dictionaries else asset_name(p)[0] == texture_stem)]
            require(model_id + index not in dictionaries or textures,
                    f'Missing YTD dictionary declared by YTYP for {original.name}')
            local = [p for p in textures if p.parent == model.parent]
            textures = local or textures
            if original.relative_to(root).as_posix() in texture_choices:
                textures = [source / texture_choices[original.relative_to(root).as_posix()]]
            # Prefer raw YTD over its adjacent XML export; ambiguity is an error.
            textures = [p for p in textures if not (p.name.lower().endswith('.xml') and
                        any(q.parent == p.parent and q.name.casefold() == p.name[:-4].casefold() for q in textures))]
            require(len(textures) <= 1, f'Ambiguous texture dictionaries for {original.name}')
            converted = work / f'converted-{index:03}'
            options = SimpleNamespace(input=model, out=converted, type=kind if skinned else 'vehicle' if kind == 'vehicles' else 'map',
                                      textures=textures[0] if textures else None, skeleton=source / skeleton if skeleton else None,
                                      base_player=rig.get('base_player'), base_ifp=rig.get('base_ifp'),
                                      base_txd=rig.get('base_txd'), bone_map=rig.get('bone_map'), scale=1., flip_v=False,
                                      enable=False, model_id=model_id + index, position=position or [0., 0., 0.])
            with contextlib.redirect_stdout(io.StringIO()):
                converter.convert(options)
            conversion = json.loads((converted / 'data/conversion-report.json').read_text(encoding='utf-8'))
            conversion['source'] = original.relative_to(root).as_posix()
            report['converted'].append(conversion)
            folder = f'stream/model-{index:03}'
            (package / folder).parent.mkdir(parents=True, exist_ok=True)
            (package / folder).mkdir()
            for filename in ('converted.dff', 'converted.txd'):
                if (converted / 'stream' / filename).exists():
                    shutil.move(str(converted / 'stream' / filename), str(package / folder / filename))
            entry = dict(dff=f'{folder}/converted.dff')
            if (package / folder / 'converted.txd').exists():
                entry['txd'] = f'{folder}/converted.txd'
            label = ''.join(ch for ch in stem if ch.isprintable())[:48].strip() or 'Custom model'
            if kind == 'player':
                manifest['name'] = label
                manifest['player'] = dict(entry, ifp='stream/rig/base.ifp')
            elif kind == 'clothing':
                manifest['player']['clothes'].append(dict(entry, name=label, enabled=True))
            elif kind == 'vehicles':
                entry['name'] = ''.join(ch for ch in stem if ch.isprintable())[:48].strip() or 'Custom car'
                manifest['vehicles'].append(entry)
            else:
                entry['id'] = model_id + index
                selected_collision = collision_choices.get(original.relative_to(root).as_posix())
                if selected_collision:
                    xml = converter.extract(source / selected_collision, work / f'collision-{index:03}')
                    faces = collision_converter.triangles(xml, converter.parse_xml)
                    data, _, _ = collision_converter.col(faces)
                    (package / folder / 'converted.col').write_bytes(data)
                    entry['col'] = f'{folder}/converted.col'
                    report['collisions'].append(dict(source=selected_collision, model=original.relative_to(root).as_posix(),
                                                     space='model-local', triangles=len(faces)))
                manifest['models'].append(entry)
                if kind == 'props':
                    point = list(position); point[0] += 3 * (index % 8); point[1] += 3 * (index // 8)
                    manifest['placements'].append(dict(model_id=entry['id'], position=point))
        if skinned:
            report['warnings'] = [warning for warning in report['warnings'] if not warning.startswith('YDD peds/clothes')]
            report['warnings'].append('Explicit bone retargeting only; fit meshes to the native rig. No GTA V facial/cloth animation or freemode component metadata.')
            report['texture_overrides'] = texture_choices
        if kind == 'map':
            map_source = converter.extract(source / ymap, work / 'ymap-xml')
            manifest['placements'], skipped = map_converter.placements(
                map_source, [p.relative_to(root) for p in chosen], model_id, offset, converter.parse_xml, aliases)
            require(len(manifest['placements']) <= 2000, 'Map exceeds native 2000 placement resource budget')
            report['map'] = dict(source=ymap, offset=list(offset), placements=len(manifest['placements']), skipped_lods=skipped, ytyp=list(ytyp), archetype_aliases=len(aliases))
            report['warnings'] = [warning for warning in report['warnings'] if not warning.startswith('YMAP/YTYP require')]
            report['warnings'].append('Only HD static CEntityDef placements imported; MLO/portal behavior remains unsupported. YBN collision is imported only when explicitly selected.')
            for index, collision in enumerate(world_collision):
                xml = converter.extract(source / collision, work / f'world-collision-{index:03}')
                faces = collision_converter.triangles(xml, converter.parse_xml)
                # Rebase world-space vertices around their bounds. An identity
                # placement at world zero would be culled by the native region
                # prefilter even when its actual collision is near the player.
                center = [(min(p[a] for face in faces for p in face) + max(p[a] for face in faces for p in face))/2
                          for a in range(3)]
                half_extents = [max(abs(p[a]-center[a]) for face in faces for p in face) for a in range(2)]
                require(sum(v*v for v in half_extents) <= 1600.0**2,
                        'World collision extends beyond native region prefilter; split into smaller YBN files')
                placement = [a+b for a,b in zip(center, offset)]
                require(all(math.isfinite(v) and abs(v) < 10000 for v in placement), 'Translated collision placement exceeds native coordinate budget')
                faces = [[tuple(v-c for v,c in zip(point,center)) for point in face] for face in faces]
                data, low, high = collision_converter.col(faces)
                folder = f'stream/collision-{index:03}'
                (package / folder).mkdir()
                (package / folder / 'world.col').write_bytes(data)
                (package / folder / 'bounds.dff').write_bytes(collision_converter.invisible_dff(low, high, converter))
                id_ = model_id + len(chosen) + index
                manifest['models'].append(dict(id=id_, dff=f'{folder}/bounds.dff', col=f'{folder}/world.col'))
                manifest['placements'].append(dict(model_id=id_, position=placement))
                report['collisions'].append(dict(source=collision, space='world', triangles=len(faces), offset=list(offset), origin=center))
            require(len(manifest['placements']) <= 2000, 'Map including collision bounds exceeds native 2000 placement resource budget')
        if report['collisions']:
            report['warnings'].append('YBN triangles/boxes baked into native COL; GTA V materials, flags, BVH, margins and dynamic physics semantics are not retained.')
        require(sum(p.stat().st_size for p in package.rglob('*') if p.is_file()) <= 128 * 1024 * 1024,
                'Converted pack exceeds 128 MiB server resource budget')
        require(sum(1 for p in package.rglob('*') if p.is_file()) + 1 <= 64,
                'Converted pack exceeds 64 shared files including resource.json; select fewer models')
        (package / 'resource.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
        (package / 'data').mkdir()
        (package / 'data/fivem-import-report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
        require(not output.exists(), 'Output appeared during import; refusing overwrite')
        package.rename(output)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('input', type=Path)
    parser.add_argument('--out', type=Path, help='New native resource directory; omit to inspect only')
    parser.add_argument('--kind', choices=('vehicles', 'props', 'map', 'player', 'clothing'), default='vehicles')
    parser.add_argument('--base-player', type=Path, help='Native target DFF rig; required for player/clothing')
    parser.add_argument('--base-ifp', type=Path, help='Native target idle/walk/run IFP; required for player/clothing')
    parser.add_argument('--base-txd', type=Path, help='Optional native base-player textures for clothing')
    parser.add_argument('--bone-map', type=Path, help='Explicit source bone names/tags to native HAnim IDs')
    parser.add_argument('--skeleton', help='Exact relative CodeWalker source skeleton XML inside this resource')
    parser.add_argument('--texture', action='append', default=[], help='Exact relative MODEL=YTD texture pairing; repeat')
    parser.add_argument('--collision', action='append', default=[], help='Explicit model-local MODEL=YBN pair; props/map only, repeat')
    parser.add_argument('--world-collision', action='append', default=[], help='Explicit world-space YBN or YBN XML path; map only, translated by --offset')
    parser.add_argument('--ymap', help='Exact relative Legacy .ymap or CodeWalker .ymap.xml path for static map placements')
    parser.add_argument('--ytyp', action='append', default=[], help='Exact relative static Legacy .ytyp or .ytyp.xml path; repeat for aliases/dictionaries')
    parser.add_argument('--offset', type=float, nargs=3, default=[0., 0., 0.], help='Translate imported map in SA world coordinates')
    parser.add_argument('--model', action='append', default=[], help='Exact relative model path; repeat to select assets')
    parser.add_argument('--position', type=float, nargs=3, help='Prop preview origin; models placed 3 m apart')
    parser.add_argument('--model-id', type=int, default=30000)
    parser.add_argument('--enable', action='store_true', help='Enable converted resource; disabled by default')
    args = parser.parse_args()
    try:
        if args.out:
            report = import_resource(args.input, args.out, args.kind, args.model, args.position, args.model_id, args.enable,
                                     args.ymap, args.offset, args.ytyp, args.base_player, args.base_ifp, args.base_txd,
                                     args.bone_map, args.skeleton, args.texture, args.collision, args.world_collision)
        else:
            _, report = inspect_resource(args.input)
        print(json.dumps(report, indent=2))
    except (ValueError, OSError, converter.ET.ParseError, converter.S.error, converter.subprocess.CalledProcessError) as error:
        parser.exit(1, f'FiveM import failed: {error}\n')


if __name__ == '__main__':
    main()
