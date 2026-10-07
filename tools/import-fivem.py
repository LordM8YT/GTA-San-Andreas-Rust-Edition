#!/usr/bin/env python3
"""Inspect a FiveM resource without executing Lua; batch-convert cars or props."""
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
        notices.append('YMAP placements, static YTYP aliases require map conversion; MLO rooms/portals and YBN collision remain unsupported.')
    if any(asset_name(p)[1] == '.ydd' for p in assets):
        notices.append('YDD peds/clothes need explicit target rig and bone mapping through convert-gta5.py.')
    metadata = [relative(p) for p in files if p.suffix.lower() == '.meta']
    if metadata:
        notices.append('GTA V handling, vehicle metadata, tuning and colours are not imported.')
    return files, dict(resource=root.name, manifest=relative(manifest),
                       assets=[relative(p) for p in assets], scripts=scripts,
                       metadata=metadata, warnings=notices)


def select_models(root, files, kind, requested=()):
    extension = '.yft' if kind == 'vehicles' else '.ydr'
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
    require(0 < len(chosen) <= (32 if kind == 'vehicles' else 64),
            'No compatible models, or native model budget exceeded (32 cars/64 props)')
    return sorted(chosen, key=lambda p: p.relative_to(root).as_posix().casefold())


def import_resource(root, output, kind, requested=(), position=None, model_id=30000, enable=False,
                    ymap=None, offset=(0., 0., 0.), ytyp=()):
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
    output.parent.mkdir(parents=True, exist_ok=True)
    # Snapshot only data into an isolated tree. XML texture discovery cannot
    # wander through the source package or follow a changed source link.
    with tempfile.TemporaryDirectory(prefix='.sa-fivem-', dir=output.parent) as work:
        work = Path(work)
        source = work / 'source'; source.mkdir()
        fingerprints = {}
        snapshot_bytes = 0
        for path in files:
            if asset_name(path)[1] in ('.yft', '.ydr', '.ytd', '.ymap', '.ytyp') or path.suffix.lower() == '.dds':
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
        manifest['vehicles' if kind == 'vehicles' else 'models'] = []
        if kind != 'vehicles':
            manifest['placements'] = []
        report['converted'] = []
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
            # Prefer raw YTD over its adjacent XML export; ambiguity is an error.
            textures = [p for p in textures if not (p.name.lower().endswith('.xml') and
                        any(q.parent == p.parent and q.name.casefold() == p.name[:-4].casefold() for q in textures))]
            require(len(textures) <= 1, f'Ambiguous texture dictionaries for {original.name}')
            converted = work / f'converted-{index:03}'
            options = SimpleNamespace(input=model, out=converted, type='vehicle' if kind == 'vehicles' else 'map',
                                      textures=textures[0] if textures else None, skeleton=None, base_player=None,
                                      base_ifp=None, base_txd=None, bone_map=None, scale=1., flip_v=False,
                                      enable=False, model_id=model_id + index, position=position or [0., 0., 0.])
            with contextlib.redirect_stdout(io.StringIO()):
                converter.convert(options)
            conversion = json.loads((converted / 'data/conversion-report.json').read_text(encoding='utf-8'))
            conversion['source'] = original.relative_to(root).as_posix()
            report['converted'].append(conversion)
            folder = f'stream/model-{index:03}'
            (package / folder).parent.mkdir(parents=True, exist_ok=True)
            shutil.move(str(converted / 'stream'), str(package / folder))
            entry = dict(dff=f'{folder}/converted.dff')
            if (package / folder / 'converted.txd').exists():
                entry['txd'] = f'{folder}/converted.txd'
            if kind == 'vehicles':
                entry['name'] = ''.join(ch for ch in stem if ch.isprintable())[:48].strip() or 'Custom car'
                manifest['vehicles'].append(entry)
            else:
                entry['id'] = model_id + index
                manifest['models'].append(entry)
                if kind == 'props':
                    point = list(position); point[0] += 3 * (index % 8); point[1] += 3 * (index // 8)
                    manifest['placements'].append(dict(model_id=entry['id'], position=point))
        if kind == 'map':
            map_source = converter.extract(source / ymap, work / 'ymap-xml')
            manifest['placements'], skipped = map_converter.placements(
                map_source, [p.relative_to(root) for p in chosen], model_id, offset, converter.parse_xml, aliases)
            require(len(manifest['placements']) <= 2000, 'Map exceeds native 2000 placement resource budget')
            report['map'] = dict(source=ymap, offset=list(offset), placements=len(manifest['placements']), skipped_lods=skipped, ytyp=list(ytyp), archetype_aliases=len(aliases))
            report['warnings'] = [warning for warning in report['warnings'] if not warning.startswith('YMAP placements,')]
            report['warnings'].append('Only HD static CEntityDef placements imported; MLO/portal/collision metadata remains unsupported.')
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
    parser.add_argument('--kind', choices=('vehicles', 'props', 'map'), default='vehicles')
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
            report = import_resource(args.input, args.out, args.kind, args.model, args.position, args.model_id, args.enable, args.ymap, args.offset, args.ytyp)
        else:
            _, report = inspect_resource(args.input)
        print(json.dumps(report, indent=2))
    except (ValueError, OSError, converter.ET.ParseError, converter.S.error, converter.subprocess.CalledProcessError) as error:
        parser.exit(1, f'FiveM import failed: {error}\n')


if __name__ == '__main__':
    main()
