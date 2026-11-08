#!/usr/bin/env python3
"""Inspect and convert classic GTA San Andreas mod folders into SARE resources.

The importer reads data files and uncompressed IMG entries as inert data. It never
executes CLEO, ASI, DLL, Lua, or other scripts from a mod package.
"""
import argparse
import json
import math
import os
from pathlib import Path
import re
import shutil
import stat
import struct
import tempfile

MAX_FILES = 4096
MAX_INPUT_BYTES = 512 * 1024 * 1024
MAX_ASSET_BYTES = 16 * 1024 * 1024
MAX_OUTPUT_BYTES = 128 * 1024 * 1024
MAX_MODELS = 256
MAX_PLACEMENTS = 2000
IMG_SECTOR = 2048
ASSET_EXTENSIONS = {'.dff', '.txd', '.col', '.ifp'}
NAME_RE = re.compile(r'^[a-z0-9_-]{1,64}$', re.IGNORECASE)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def checked_tree(root):
    """Reject links and special files before reading any source asset."""
    require(root.is_dir(), 'Input must be an extracted GTA San Andreas mod folder')
    info = root.lstat()
    require(not stat.S_ISLNK(info.st_mode) and not getattr(info, 'st_file_attributes', 0) & 0x400,
            'Input root must not be a link or junction')
    root = root.resolve()
    files = []
    total = 0
    seen = 0

    def visit(directory, depth):
        nonlocal total, seen
        require(depth <= 12, 'Mod folder nesting exceeds 12 levels')
        with os.scandir(directory) as entries:
            children = sorted(entries, key=lambda entry: entry.name.casefold())
        for entry in children:
            seen += 1
            require(seen <= MAX_FILES, 'Mod folder contains too many entries')
            item = entry.stat(follow_symlinks=False)
            require(not stat.S_ISLNK(item.st_mode) and not getattr(item, 'st_file_attributes', 0) & 0x400,
                    f'Links and junctions are unsupported: {entry.name}')
            path = Path(entry.path)
            require(path.resolve().is_relative_to(root), 'Mod asset escapes the input folder')
            if stat.S_ISDIR(item.st_mode):
                visit(path, depth + 1)
            else:
                require(stat.S_ISREG(item.st_mode), f'Unsupported file type: {entry.name}')
                require(item.st_size <= 128 * 1024 * 1024,
                        f'Input file exceeds 128 MiB: {entry.name}')
                total += item.st_size
                require(total <= MAX_INPUT_BYTES, 'Mod folder exceeds the 512 MiB input budget')
                files.append(path)

    visit(root, 0)
    return root, files


def parse_img(path):
    """Index supported uncompressed VER2 IMG entries without extracting them."""
    length = path.stat().st_size
    with path.open('rb') as stream:
        header = stream.read(8)
        require(len(header) == 8 and header[:4] == b'VER2',
                f'{path.name}: only GTA SA VER2 IMG archives are supported')
        count = struct.unpack_from('<I', header, 4)[0]
        directory_end = 8 + count * 32
        require(count <= 200000 and directory_end <= length,
                f'{path.name}: invalid IMG directory bounds')
        directory = stream.read(count * 32)
        require(len(directory) == count * 32, f'{path.name}: truncated IMG directory')
        entries = {}
        compressed = 0
        for offset in range(0, len(directory), 32):
            row = directory[offset:offset + 32]
            sector, sectors, archived = struct.unpack_from('<IHH', row, 0)
            raw_name = row[8:32].split(b'\0', 1)[0]
            if not raw_name:
                continue
            try:
                name = raw_name.decode('ascii').replace('\\', '/').rsplit('/', 1)[-1].casefold()
            except UnicodeDecodeError as error:
                raise ValueError(f'{path.name}: IMG contains a non-ASCII entry name') from error
            require(name not in ('', '.', '..') and '/' not in name,
                    f'{path.name}: invalid IMG entry name')
            data_offset = sector * IMG_SECTOR
            data_size = sectors * IMG_SECTOR
            if archived != 0 or sectors == 0 or data_size > MAX_ASSET_BYTES:
                compressed += 1
                continue
            require(data_offset >= directory_end and data_offset + data_size <= length,
                    f'{path.name}: IMG entry {name} is outside the archive')
            entries[name] = (path, data_offset, data_size)
    return entries, compressed


def read_asset(source):
    if isinstance(source, tuple):
        path, offset, size = source
        with path.open('rb') as stream:
            stream.seek(offset)
            data = stream.read(size)
        require(len(data) == size, f'Truncated IMG asset in {path.name}')
        return data
    require(source.stat().st_size <= MAX_ASSET_BYTES,
            f'Asset exceeds 16 MiB: {source.name}')
    return source.read_bytes()


def asset_index(root, files):
    """Index loose RenderWare files and supported IMG archive entries."""
    assets = {}
    duplicate_names = []
    compressed_entries = 0
    for path in files:
        suffix = path.suffix.casefold()
        if suffix == '.img':
            entries, skipped = parse_img(path)
            compressed_entries += skipped
            for name, source in sorted(entries.items()):
                if name in assets:
                    duplicate_names.append(name)
                assets[name] = source
        elif suffix in ASSET_EXTENSIONS:
            name = path.name.casefold()
            if name in assets:
                duplicate_names.append(name)
            assets[name] = path
    return assets, compressed_entries, sorted(set(duplicate_names))


def data_lines(path):
    require(path.stat().st_size <= MAX_ASSET_BYTES,
            f'Data file exceeds 16 MiB: {path.name}')
    text = path.read_text(encoding='utf-8-sig', errors='replace')
    for raw in text.splitlines():
        line = raw.split('#', 1)[0].strip()
        if line:
            yield line


def parse_ide(paths):
    """Read standard IDE model declarations and vehicle model-to-TXD pairs."""
    definitions = {}
    cars = {}
    unsupported_vehicle_types = {}
    for path in sorted(paths, key=lambda p: p.as_posix().casefold()):
        section = ''
        for line in data_lines(path):
            if ',' not in line:
                section = '' if line.casefold() == 'end' else line.casefold()
                continue
            fields = [field.strip() for field in line.split(',')]
            if section in ('objs', 'tobj', 'anim') and len(fields) >= 3:
                try:
                    model_id = int(fields[0], 10)
                except ValueError:
                    continue
                model, txd = fields[1].casefold(), fields[2].casefold()
                if 0 <= model_id <= 2_147_483_647 and NAME_RE.fullmatch(model) and NAME_RE.fullmatch(txd):
                    definitions[model_id] = (model, txd)
            elif section == 'cars' and len(fields) >= 5:
                model, txd, vehicle_type = fields[1].casefold(), fields[2].casefold(), fields[3].casefold()
                if NAME_RE.fullmatch(model) and NAME_RE.fullmatch(txd):
                    if vehicle_type == 'car':
                        cars[model] = txd
                    else:
                        unsupported_vehicle_types[model] = vehicle_type
    return definitions, cars, unsupported_vehicle_types


def vehicle_handling_ids(paths):
    """Map car model basenames to their handling.cfg identifiers."""
    result = {}
    for path in sorted(paths, key=lambda p: p.as_posix().casefold()):
        section = ''
        for line in data_lines(path):
            if ',' not in line:
                section = '' if line.casefold() == 'end' else line.casefold()
                continue
            if section != 'cars':
                continue
            fields = [field.strip() for field in line.split(',')]
            if len(fields) < 5:
                continue
            model, vehicle_type, handling_id = fields[1].casefold(), fields[3].casefold(), fields[4].casefold()
            if vehicle_type == 'car' and NAME_RE.fullmatch(model) and NAME_RE.fullmatch(handling_id):
                result[model] = handling_id
    return result


def native_handling_from_line(line):
    """Translate the same standard GTA SA handling.cfg columns as SARE's Rust loader."""
    fields = line.split()
    if len(fields) < 33:
        return None
    try:
        values = [float(fields[index]) for index in (1, 2, 3, 5, 6, 8, 9, 10, 12, 13, 14, 17, 18, 20)]
    except ValueError:
        return None
    if not all(math.isfinite(value) for value in values):
        return None
    (mass, turn_mass, drag, centre_x, centre_z, traction, traction_loss,
     traction_bias, max_velocity, engine_acceleration, inertia,
     brake_deceleration, brake_bias, steering_lock) = values
    mass = max(1.0, mass)
    turn_mass = max(1.0, turn_mass)
    drag = max(0.0, drag)
    inertia = max(0.0, inertia)

    acceleration = (0.34 * engine_acceleration / (1.0 + inertia / 50.0)
                    * max(0.4, min(1.1, (1500.0 / mass) ** 0.3)))
    acceleration = max(0.5, min(20.0, acceleration))
    target = max(5.0, min(85.0, 0.9 * max_velocity / 3.6))
    aerodynamic_drag = max(0.0, min(0.02, 0.0004 * drag))
    spare = lambda coefficient: acceleration - 0.28 - coefficient * target * target
    if spare(aerodynamic_drag) < 0.1 * acceleration:
        aerodynamic_drag = max(0.0, (0.9 * acceleration - 0.28) / (target * target))
    top_speed = max(5.0, min(90.0, target * math.sqrt(acceleration / max(spare(aerodynamic_drag), 0.01))))
    try:
        flags = int(fields[32], 16)
    except ValueError:
        flags = 0
    drive = 1.0 if fields[15].upper() == 'R' else 2.0 if fields[15] == '4' else 0.0
    profile = {
        'acceleration': acceleration,
        'reverse_acceleration': max(0.5, min(10.0, 0.6 * acceleration)),
        'brake_deceleration': max(1.0, min(25.0, 1.2 * brake_deceleration)),
        'top_speed': top_speed,
        'reverse_speed': max(1.0, min(12.0, 0.25 * target)),
        'tire_grip': max(4.2, min(8.5, 6.0 * math.sqrt(max(0.0, traction / 0.75)))),
        'steering_lock': max(0.3, min(0.9, math.radians(steering_lock))),
        'steering_rate': 3.5,
        'steer_fade_speed': 22.0,
        'aerodynamic_drag': aerodynamic_drag,
        'traction_loss': max(0.6, min(0.95, 0.4 + 0.55 * traction_loss)),
        'front_grip': max(0.4, min(1.6, 1.0 + 1.2 * (traction_bias - 0.5))),
        'drive': drive,
        'brake_front': max(0.2, min(0.8, brake_bias)),
        'engine_lag': max(0.0, min(1.5, 0.012 * inertia)),
        'yaw_inertia': max(0.5, min(4.0, turn_mass / (mass * 3.6))),
        'front_weight': max(0.35, min(0.65, 0.5 + centre_x / 2.7)),
        'cg_offset': max(-0.3, min(0.3, centre_z)),
        'handbrake': 0.0 if flags & 0x10 else 1.0,
        'air_control': 1.5,
    }
    return profile


def handling_profiles(paths, model_to_id):
    profiles = {}
    requested = set(model_to_id.values())
    for path in sorted(paths, key=lambda p: p.as_posix().casefold()):
        if path.name.casefold() != 'handling.cfg':
            continue
        for line in data_lines(path):
            handling_id = line.split()[0].casefold()
            if handling_id not in requested or handling_id in profiles:
                continue
            profile = native_handling_from_line(line)
            if profile is not None:
                profiles[handling_id] = profile
    return {model: profiles[handling_id] for model, handling_id in model_to_id.items()
            if handling_id in profiles}


def parse_ipl(paths):
    placements = []
    malformed = 0
    binary = []
    for path in sorted(paths, key=lambda p: p.as_posix().casefold()):
        if path.suffix.casefold() != '.ipl':
            continue
        with path.open('rb') as stream:
            if stream.read(4) == b'bnry':
                binary.append(path.name)
                continue
        section = ''
        for line in data_lines(path):
            if ',' not in line:
                section = '' if line.casefold() == 'end' else line.casefold()
                continue
            if section != 'inst':
                continue
            fields = [field.strip() for field in line.split(',')]
            if len(fields) != 11:
                malformed += 1
                continue
            try:
                model_id = int(fields[0], 10)
                interior = int(fields[2], 10)
                position = [float(fields[i]) for i in (3, 4, 5)]
                rotation = [float(fields[i]) for i in (6, 7, 8, 9)]
                lod = int(fields[10], 10)
            except ValueError:
                malformed += 1
                continue
            norm = sum(value * value for value in rotation)
            if model_id < 0 or not 0 <= interior <= 255 or not all(math.isfinite(v) for v in position + rotation):
                malformed += 1
                continue
            if not (0.98 <= norm <= 1.02) or any(abs(v) > 10000 for v in position):
                malformed += 1
                continue
            placements.append({
                'model_id': model_id,
                'position': position,
                'rotation': rotation,
                '_interior': interior,
                '_lod': lod,
            })
            require(len(placements) <= 100000, 'IPL contains too many placement records')
    return placements, malformed, binary


def installed_ide_paths(game_root):
    """Read IDE files registered by the installed game, matching the runtime."""
    root = Path(game_root).resolve()
    paths = []
    for manifest_name in ('default.dat', 'gta.dat'):
        manifest = root / 'data' / manifest_name
        require(manifest.is_file() and not manifest.is_symlink(),
                f'Installed game is missing a safe data/{manifest_name}')
        for line in data_lines(manifest):
            fields = line.split()
            if len(fields) < 2 or fields[0].casefold() != 'ide' or not fields[1].casefold().endswith('.ide'):
                continue
            relative = Path(fields[1].replace('\\', '/'))
            require(not relative.is_absolute() and '..' not in relative.parts,
                    f'Unsafe IDE path in data/{manifest_name}: {fields[1]}')
            path = root.joinpath(*relative.parts)
            info = path.lstat()
            require(stat.S_ISREG(info.st_mode) and not stat.S_ISLNK(info.st_mode)
                    and not getattr(info, 'st_file_attributes', 0) & 0x400,
                    f'Unsafe installed IDE file: {fields[1]}')
            resolved = path.resolve()
            require(resolved.is_relative_to(root),
                    f'Installed IDE path escapes the game folder: {fields[1]}')
            require(info.st_size <= MAX_ASSET_BYTES,
                    f'Installed IDE file exceeds 16 MiB: {path.name}')
            paths.append(resolved)
            require(len(paths) <= 128, 'Too many registered IDE files')
    return paths

class Exporter:
    def __init__(self, output):
        self.output = output
        self.total_bytes = 0
        self.asset_paths = {}
        self.files = []

    def save_asset(self, assets, name, extension, destination):
        key = f'{name.casefold()}{extension}'
        source = assets.get(key)
        if source is None:
            return None
        if key in self.asset_paths:
            return self.asset_paths[key]
        data = read_asset(source)
        self.total_bytes += len(data)
        require(self.total_bytes <= MAX_OUTPUT_BYTES, 'Converted resource exceeds 128 MiB')
        self.output.joinpath(destination).parent.mkdir(parents=True, exist_ok=True)
        self.output.joinpath(destination).write_bytes(data)
        self.asset_paths[key] = destination
        self.files.append(destination)
        return destination


def import_resource(root, output, kind='auto', models=(), model_id=30000,
                    position=None, name=None, enable=False, game_dir=None):
    root, files = checked_tree(root)
    output = output.absolute()
    require(not output.exists(), 'Output already exists; choose a new directory')
    require(not output.resolve().is_relative_to(root), 'Output must be outside the source mod folder')
    require(0 <= model_id <= 2_147_483_647, 'Invalid starting model ID')
    require(position is None or (len(position) == 3 and all(math.isfinite(v) and abs(v) <= 10000 for v in position)),
            'Position must contain three finite world coordinates')
    display_name = ' '.join((name or root.name).split())
    require(1 <= len(display_name) <= 96 and all(ord(char) >= 32 and ord(char) != 127 for char in display_name),
            'Resource name must be 1-96 printable characters')
    wanted = {item.casefold() for item in models}
    require(all(NAME_RE.fullmatch(item) for item in wanted), 'Model names must use 1-64 letters, digits, underscores or hyphens')

    assets, compressed_entries, duplicates = asset_index(root, files)
    ide_paths = [path for path in files if path.suffix.casefold() == '.ide']
    ipl_paths = [path for path in files if path.suffix.casefold() == '.ipl']
    definitions, cars, unsupported_vehicle_types = parse_ide(ide_paths)
    game_definitions = {}
    original_txd_names = set()
    if game_dir:
        game_root = Path(game_dir).absolute()
        require(game_root.is_dir(), '--game-dir must point to an installed GTA San Andreas folder')
        base_ide = installed_ide_paths(game_root)
        game_definitions, _, _ = parse_ide(base_ide)
        for archive_name in ('gta3.img', 'gta_int.img'):
            archive_path = game_root / 'models' / archive_name
            if not archive_path.exists():
                if archive_name == 'gta3.img':
                    raise ValueError('--game-dir is missing models/gta3.img')
                continue
            require(not archive_path.is_symlink(), f'Unsafe original game archive: {archive_name}')
            archive_entries, _ = parse_img(archive_path)
            original_txd_names.update(
                Path(entry).stem for entry in archive_entries if entry.endswith('.txd')
            )
    placements, malformed_placements, binary_ipls = parse_ipl(ipl_paths)

    car_handling_ids = vehicle_handling_ids(ide_paths)
    custom_handling = handling_profiles(files, car_handling_ids)

    if kind == 'auto':
        if cars and any(f'{model}.dff' in assets for model in cars):
            kind = 'vehicles'
        elif definitions:
            kind = 'map'
        elif any(key.endswith('.dff') for key in assets) and not unsupported_vehicle_types:
            kind = 'vehicles'
        else:
            raise ValueError('No supported DFF assets or IDE model declarations were found')

    parent = output.parent
    parent.mkdir(parents=True, exist_ok=True)
    stage = Path(tempfile.mkdtemp(prefix=f'.{output.name}.sare-', dir=parent))
    try:
        exporter = Exporter(stage)
        manifest = {
            'schema_version': 2,
            'enabled': bool(enable),
            'name': display_name,
        }
        imported = []
        omitted = []

        if kind == 'vehicles':
            if not cars and unsupported_vehicle_types:
                types = ', '.join(sorted(set(unsupported_vehicle_types.values())))
                raise ValueError(f'No passenger-car models were found; SARE custom vehicles currently use car physics (found: {types})')
            candidates = cars if cars else {
                Path(key).stem: Path(key).stem
                for key in assets if key.endswith('.dff')
            }
            selected = [model for model in sorted(candidates) if not wanted or model in wanted]
            require(bool(selected), 'No selected vehicle DFFs found in the mod folder or IMG archives')
            require(len(selected) <= 32, 'Native vehicle limit exceeded (32); select models with --model')
            vehicles = []
            for index, model in enumerate(selected, 1):
                dff = exporter.save_asset(assets, model, '.dff', f'stream/vehicle-{index:03}.dff')
                if not dff:
                    omitted.append(f'{model}: missing DFF')
                    continue
                txd = exporter.save_asset(assets, candidates[model], '.txd', f'stream/vehicle-{index:03}.txd')
                vehicle_row = {
                    'name': model,
                    'dff': dff,
                    **({'txd': txd} if txd else {}),
                    **({'original_txd': candidates[model]} if not txd and candidates[model] in original_txd_names else {}),
                    **({'handling': custom_handling[model]} if model in custom_handling else {}),
                }
                vehicles.append(vehicle_row)
                imported.append(model)
            require(bool(vehicles), 'No compatible vehicles could be imported')
            manifest['vehicles'] = vehicles

        elif kind == 'player':
            dff_names = sorted(Path(key).stem for key in assets if key.endswith('.dff'))
            selected = [model for model in dff_names if not wanted or model in wanted]
            require(len(selected) == 1, 'Player import needs exactly one DFF; choose it with --model')
            model = selected[0]
            dff = exporter.save_asset(assets, model, '.dff', 'stream/player.dff')
            txd = exporter.save_asset(assets, model, '.txd', 'stream/player.txd')
            ifp = exporter.save_asset(assets, model, '.ifp', 'stream/player.ifp')
            manifest['player'] = {
                'dff': dff,
                **({'txd': txd} if txd else {}),
                **({'ifp': ifp} if ifp else {}),
            }
            imported.append(model)

        else:
            require(kind in ('map', 'props'), 'Unsupported kind')
            if kind == 'props':
                require(position is not None, 'Props require --position X Y Z')
                candidates = sorted(Path(key).stem for key in assets if key.endswith('.dff'))
                selected = [model for model in candidates if not wanted or model in wanted]
                require(bool(selected), 'No selected DFF props found')
                require(len(selected) <= MAX_MODELS, 'Native model limit exceeded (256)')
                require(model_id + len(selected) - 1 <= 2_147_483_647,
                        'Generated model IDs exceed the supported integer range')
                model_rows = []
                placement_rows = []
                for index, model in enumerate(selected):
                    assigned_id = model_id + index
                    dff = exporter.save_asset(assets, model, '.dff', f'stream/model-{index + 1:03}.dff')
                    txd = exporter.save_asset(assets, model, '.txd', f'stream/model-{index + 1:03}.txd')
                    col = exporter.save_asset(assets, model, '.col', f'stream/model-{index + 1:03}.col')
                    model_rows.append({'id': assigned_id, 'dff': dff,
                                       **({'txd': txd} if txd else {}),
                                       **({'col': col} if col else {})})
                    placement_rows.append({'model_id': assigned_id,
                                           'position': [position[0] + index * 3.0, position[1], position[2]]})
                    imported.append(model)
                manifest['models'] = model_rows
                manifest['placements'] = placement_rows
            else:
                model_rows = []
                imported_ids = set()
                for model_id_value, (model, txd_name) in sorted(definitions.items()):
                    if wanted and model not in wanted:
                        continue
                    if f'{model}.dff' not in assets:
                        continue
                    index = len(model_rows) + 1
                    dff = exporter.save_asset(assets, model, '.dff', f'stream/model-{index:03}.dff')
                    txd = exporter.save_asset(assets, txd_name, '.txd', f'stream/dictionary-{len(exporter.asset_paths) + 1:03}.txd')
                    col = exporter.save_asset(assets, model, '.col', f'stream/collision-{index:03}.col')
                    row = {'id': model_id_value, 'dff': dff,
                           **({'txd': txd} if txd else {}),
                           **({'original_txd': txd_name} if not txd and txd_name in original_txd_names else {}),
                           **({'col': col} if col else {})}
                    model_rows.append(row)
                    imported_ids.add(model_id_value)
                    imported.append(model)
                    require(len(model_rows) <= MAX_MODELS, 'Native model limit exceeded (256)')
                require(bool(model_rows), 'No DFF assets matched the selected IDE model declarations')

                known_ids = set(game_definitions) | imported_ids
                placement_rows = []
                skipped_unknown = 0
                imported_interiors = 0
                for item in placements:
                    if item['model_id'] not in known_ids:
                        skipped_unknown += 1
                        continue
                    placement_rows.append({
                        'model_id': item['model_id'],
                        'interior': item['_interior'],
                        'position': item['position'],
                        'rotation': item['rotation'],
                    })
                    imported_interiors += int(item['_interior'] != 0)
                    require(len(placement_rows) <= MAX_PLACEMENTS,
                            'Native placement limit exceeded (2000); split the map resource')
                manifest['models'] = model_rows
                if placement_rows:
                    manifest['placements'] = placement_rows

        manifest_path = stage / 'resource.json'
        manifest_path.write_text(json.dumps(manifest, indent=2, allow_nan=False) + '\n', encoding='utf-8')
        # A standard FiveM-style resource manifest lets the SARE server discover
        # and ensure the same resource alongside its native resource.json.
        fx_description = manifest['name'].replace('\\', '\\\\').replace("'", "\\'")
        (stage / 'fxmanifest.lua').write_text(
            "fx_version 'cerulean'\n"
            "game 'gta3'\n"
            f"description '{fx_description}'\n",
            encoding='utf-8')
        (stage / 'README.md').write_text(
            f"# {manifest['name']}\n\n"
            "Imported GTA San Andreas assets for SARE. The resource is disabled by default.\n\n"
            "Review resource.json, confirm the model IDs and placements, then set enabled to true\n"
            "and restart the client, or add ensure <resource-folder> to a SARE server's server.cfg.\n\n"
            "Only supported data assets were imported. Original scripts and executable plugins are not run.\n",
            encoding='utf-8')
        # Re-check the generated package against the native resource byte budget.
        total = sum(path.stat().st_size for path in stage.rglob('*') if path.is_file())
        require(total <= MAX_OUTPUT_BYTES, 'Converted resource exceeds 128 MiB')
        os.replace(stage, output)
        report = {
            'resource': manifest['name'],
            'kind': kind,
            'enabled': manifest['enabled'],
            'models': imported,
            'files': len(exporter.files),
            'output_bytes': total,
            'skipped_unknown_ipl_placements': locals().get('skipped_unknown', 0),
            'imported_interior_ipl_placements': locals().get('imported_interiors', 0),
            'malformed_ipl_placements': malformed_placements,
            'compressed_img_entries': compressed_entries,
            'duplicate_asset_names': duplicates,
            'omitted': omitted,
            'binary_ipl_files': binary_ipls,
            'unsupported_vehicle_types': unsupported_vehicle_types,
        }
        return report
    except Exception:
        shutil.rmtree(stage, ignore_errors=True)
        raise


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('input', type=Path, help='Extracted GTA SA mod or Mod Loader folder')
    parser.add_argument('--out', type=Path, required=True, help='New SARE resource folder to create')
    parser.add_argument('--kind', choices=('auto', 'map', 'props', 'vehicles', 'player'), default='auto')
    parser.add_argument('--model', action='append', default=[], help='Model basename to import; repeat to select several')
    parser.add_argument('--model-id', type=int, default=30000, help='Starting ID for standalone props (default: 30000)')
    parser.add_argument('--position', nargs=3, type=float, metavar=('X', 'Y', 'Z'),
                        help='World position for standalone props')
    parser.add_argument('--game-dir', type=Path, help='Own GTA SA installation; retains IPL placements that use original model IDs')
    parser.add_argument('--name', help='Display name in the SARE resource menu')
    parser.add_argument('--enable', action='store_true', help='Enable the resource immediately instead of requiring review')
    args = parser.parse_args()
    try:
        report = import_resource(args.input, args.out, args.kind, args.model, args.model_id,
                                 args.position, args.name, args.enable, args.game_dir)
    except (OSError, ValueError, struct.error) as error:
        parser.error(str(error))
    print(json.dumps(report, indent=2))
    print(f"Created {args.out}. Review the generated resource before enabling it.")


if __name__ == '__main__':
    main()
