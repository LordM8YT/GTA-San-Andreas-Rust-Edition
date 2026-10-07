"""Read-only SA placement probe. No geometry, textures or original metadata exported."""
import argparse
import json
import math
from pathlib import Path
import struct

from inspect_installation import img_index, ide_objects, text_lines, SECTOR


def instance(model_id, interior, position, rotation, lod, source, index, model=''):
    if model_id < 0 or interior < 0 or lod < -1:
        raise ValueError('Invalid placement identifiers')
    if not all(math.isfinite(v) for v in (*position, *rotation)):
        raise ValueError('Non-finite placement')
    if not .98 <= sum(v*v for v in rotation) <= 1.02:
        raise ValueError('Placement quaternion is not unit length')
    return dict(id=model_id, interior=interior, position=list(position),
                rotation=list(rotation), lod=lod, source=source, index=index, model=model)


def binary_instances(data, source):
    if len(data) < 32 or data[:4] != b'bnry':
        raise ValueError('Invalid binary IPL header')
    count = struct.unpack_from('<I', data, 4)[0]
    offset = struct.unpack_from('<I', data, 28)[0]
    if count > 200000 or offset < 32 or offset + count*40 > len(data):
        raise ValueError('Binary IPL instances outside bounds')
    rows = []
    for i in range(count):
        values = struct.unpack_from('<7f3i', data, offset + i*40)
        rows.append(instance(values[7], values[8], values[:3], values[3:7],
                             values[9], source, i))
    return rows


def text_instances(path, source):
    section = ''
    rows = []
    for number, text in text_lines(path):
        if ',' not in text:
            section = '' if text.lower() == 'end' else text.lower()
            continue
        if section != 'inst':
            continue
        f = [s.strip() for s in text.split(',')]
        if len(f) != 11:
            raise ValueError(f'Unsupported text IPL record: {source}:{number}')
        rows.append(instance(int(f[0]), int(f[2]), tuple(map(float,f[3:6])),
                             tuple(map(float,f[6:10])), int(f[10]), source, len(rows), f[1]))
    return rows


class Archive:
    def __init__(self, path):
        self.path = path
        index = img_index(path)
        self.entries = {}
        for e in index['entries']:
            key = e['name'].lower()
            if key in self.entries:
                raise ValueError(f'Duplicate archive name: {key}')
            self.entries[key] = e

    def read(self, name):
        e = self.entries[name.lower()]
        size = e['streaming_sectors'] * SECTOR
        if not e['classic_range_valid'] or e['archive_sectors'] or size > 16*1024*1024:
            raise ValueError(f'Unsupported archive range: {name}')
        with self.path.open('rb') as f:
            f.seek(e['sector_offset']*SECTOR)
            data = f.read(size)
        if len(data) != size:
            raise ValueError('Truncated archive payload')
        return data


def safe_files(root, suffix):
    for p in sorted((root/'data').rglob('*')):
        if p.suffix.lower() == suffix and not p.is_symlink() and p.resolve().is_relative_to(root):
            yield p


def registered_files(root, suffix):
    """Follow the game's manifests; stray map files are not runtime inputs."""
    available = {p.relative_to(root).as_posix().lower(): p for p in safe_files(root, suffix)}
    kind = 'IDE' if suffix == '.ide' else 'IPL'
    for manifest_name in ('default.dat', 'gta.dat'):
        manifest = root / 'data' / manifest_name
        if not manifest.is_file() or manifest.is_symlink():
            raise ValueError(f'Missing safe load manifest: {manifest_name}')
        for number, line in text_lines(manifest):
            directive = line.split(None, 1)
            if len(directive) != 2 or directive[0].upper() != kind:
                continue
            relative = directive[1].replace('\\', '/').lower()
            if not relative.endswith(suffix):
                continue
            if relative not in available:
                raise ValueError(f'Missing registered {kind}: {manifest_name}:{number}: {relative}')
            yield available[relative]


def region(game, center, radius):
    if len(center) != 2 or not all(math.isfinite(v) for v in center) or not 0 < radius <= 2000:
        raise ValueError('Invalid region bounds')
    game = game.resolve(strict=True)
    archive_path = game/'models/gta3.img'
    if archive_path.is_symlink() or not archive_path.resolve().is_relative_to(game):
        raise ValueError('Archive must stay inside installation')
    archive = Archive(archive_path)
    definitions = {}
    for path in registered_files(game, '.ide'):
        for obj in ide_objects(path):
            if obj['section'] in ('objs', 'tobj'):
                # Later registered IDEs replace earlier definitions for the same ID.
                definitions[obj['id']] = obj
    rows, sources = [], []
    for path in registered_files(game, '.ipl'):
        source = path.relative_to(game).as_posix()
        sources.append(source)
        rows.extend(text_instances(path, source))
        # Streamed IPLs belong to the matching text IPL; LOD indices stay source-local.
        prefix = path.stem.lower() + '_stream'
        for name in sorted(archive.entries):
            if name.startswith(prefix) and name.endswith('.ipl') and name[len(prefix):-4].isdigit():
                sources.append(name)
                rows.extend(binary_instances(archive.read(name), name))
    selected = []
    for row in rows:
        x,y,_ = row['position']
        if row['interior'] != 0 or (x-center[0])**2 + (y-center[1])**2 > radius**2:
            continue
        obj = definitions.get(row['id'])
        dff = (obj['model']+'.dff').lower() if obj else None
        txd = (obj['txd']+'.txd').lower() if obj else None
        selected.append({**row, 'definition':obj, 'dff':dff, 'txd':txd,
                         'dff_present':dff in archive.entries, 'txd_present':txd in archive.entries})
    archive.definitions = definitions
    return archive, selected, dict(archive_entries=len(archive.entries), placement_sources=len(sources),
                                  total_placements=len(rows), definitions=len(definitions),
                                  selected_placements=len(selected),
                                  resolved_placements=sum(bool(x['definition']) for x in selected),
                                  asset_pairs_present=sum(x['dff_present'] and x['txd_present'] for x in selected))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game-dir', type=Path, required=True)
    parser.add_argument('--center', type=float, nargs=2, default=[2500,-1670])
    parser.add_argument('--radius', type=float, default=120)
    args = parser.parse_args()
    try:
        _, _, summary = region(args.game_dir, args.center, args.radius)
        print(json.dumps(summary, indent=2))
    except (ValueError, OSError, KeyError) as exc:
        parser.exit(1, f'Cannot read world metadata: {exc}\n')


if __name__ == '__main__':
    main()
