"""Read-only GTA SA inventory. Standard library only; no original assets exported."""
from __future__ import annotations

import argparse
from collections import Counter
import json
import os
from pathlib import Path
import struct
import sys
import zipfile

SECTOR = 2048
MAX_ENTRIES = 200_000
RELEVANT = {'.img', '.dir', '.ide', '.ipl', '.dff', '.txd', '.col', '.ifp', '.scm', '.dat', '.pak'}


def img_index(path: Path) -> dict:
    """Read only a VER2 directory, never asset payloads. Preserve both size fields."""
    length = path.stat().st_size
    with path.open('rb') as stream:
        header = stream.read(8)
        if len(header) != 8 or header[:4] != b'VER2':
            raise ValueError('Not a classic VER2 IMG; unsupported variant')
        count = struct.unpack_from('<I', header, 4)[0]
        end = 8 + 32 * count
        if count > MAX_ENTRIES or end > length:
            raise ValueError('Invalid or excessive IMG directory length')
        entries = []
        for i in range(count):
            offset, streaming, archived, raw_name = struct.unpack('<IHH24s', stream.read(32))
            name = raw_name.split(b'\0', 1)[0].decode('ascii', errors='replace')
            # The audit does not extract entries or interpret compressed variants.
            # For classic uncompressed PC entries, the second uint16 is normally zero.
            size = streaming or archived
            valid = offset * SECTOR >= end and (offset + size) * SECTOR <= length and size > 0
            entries.append({'name': name, 'sector_offset': offset,
                            'streaming_sectors': streaming, 'archive_sectors': archived,
                            'classic_range_valid': valid})
        return {'format': 'VER2', 'count': count, 'entries': entries,
                'nonzero_archive_size_fields': sum(e['archive_sectors'] != 0 for e in entries),
                'invalid_ranges': sum(not e['classic_range_valid'] for e in entries)}


def text_lines(path: Path):
    if path.stat().st_size > 16 * 1024 * 1024:
        raise ValueError('Metadata exceeds 16 MiB limit')
    with path.open('r', encoding='cp1252', errors='replace') as stream:
        for number, line in enumerate(stream, 1):
            text = line.split('#', 1)[0].strip()
            if text:
                yield number, text


def ide_objects(path: Path) -> list[dict]:
    section = ''
    objects = []
    for line, text in text_lines(path):
        lower = text.lower()
        if lower == 'end':
            section = ''
        elif ',' not in text:
            section = lower
        elif section in ('objs', 'tobj', 'cars', 'peds', 'weap'):
            fields = [x.strip() for x in text.split(',')]
            if len(fields) >= 3:
                try:
                    model_id = int(fields[0])
                except ValueError:
                    continue
                objects.append({'id': model_id, 'model': fields[1], 'txd': fields[2],
                                'section': section, 'line': line})
    return objects


def safe_output(game: Path, output: Path) -> tuple[Path, Path]:
    game = game.resolve(strict=True)
    output = output.resolve()
    if not game.is_dir():
        raise ValueError('Game path must be a directory')
    if game == output or game in output.parents or output in game.parents:
        raise ValueError('Output must be separate from the game directory')
    if output.exists():
        raise ValueError('Output already exists; choose a new output directory')
    return game, output


def inspect(game: Path, output: Path) -> Path:
    game, output = safe_output(game, output)
    errors, files, archives, objects, directives = [], [], [], [], []
    # No links are followed, including file links resolving outside the installation.
    for base, dirs, names in os.walk(game, followlinks=False):
        dirs[:] = sorted(d for d in dirs if not (Path(base) / d).is_symlink()
                         and (Path(base) / d).resolve().is_relative_to(game))
        for name in sorted(names):
            path = Path(base) / name
            if path.is_symlink() or not path.resolve().is_relative_to(game):
                continue
            suffix = path.suffix.lower()
            if suffix not in RELEVANT and name.lower() not in ('gta_sa.exe', 'gta-sa.exe'):
                continue
            relative = path.relative_to(game).as_posix()
            try:
                files.append({'path': relative, 'bytes': path.stat().st_size})
                if suffix == '.img':
                    archives.append({'path': relative, **img_index(path)})
                elif suffix == '.ide':
                    objects.extend({'path': relative, **obj} for obj in ide_objects(path))
                elif name.lower() in ('gta.dat', 'default.dat'):
                    for line, text in text_lines(path):
                        parts = text.split(None, 1)
                        if len(parts) == 2 and parts[0].upper() in ('IMG', 'IDE', 'IPL', 'COLFILE', 'TEXDICTION'):
                            directives.append({'source': relative, 'line': line,
                                               'kind': parts[0].upper(), 'value': parts[1]})
            except (OSError, ValueError, struct.error) as exc:
                errors.append({'path': relative, 'error': str(exc)})
    by_name = {}
    for archive in archives:
        for entry in archive['entries']:
            if entry['classic_range_valid'] and entry['archive_sectors'] == 0:
                by_name.setdefault(entry['name'].lower(), []).append(archive['path'])
    for file in files:
        if Path(file['path']).suffix.lower() in ('.dff', '.txd'):
            by_name.setdefault(Path(file['path']).name.lower(), []).append(file['path'])
    candidates = []
    for obj in objects:
        if obj['section'] not in ('objs', 'tobj'):
            continue
        dff, txd = obj['model'].lower() + '.dff', obj['txd'].lower() + '.txd'
        if dff in by_name and txd in by_name:
            candidates.append({**obj, 'dff_locations': by_name[dff], 'txd_locations': by_name[txd]})
    counts = Counter(Path(f['path']).suffix.lower() for f in files)
    report = {'schema_version': 1, 'tool': 'sa-runtime-poc read-only audit',
              'ownership_verified': False,
              'classic_markers': {'gta_dat': any(f['path'].lower() == 'data/gta.dat' for f in files),
                                  'ver2_archives': len(archives)},
              'file_counts': dict(counts), 'files': files, 'errors': errors,
              'note': 'Inventory only. No renderer, DFF mesh decoder, TXD decoder, or binary IPL loader yet.'}
    output.mkdir(parents=True, exist_ok=False)
    for name, data in [('inventory.json', report), ('img-indexes.json', archives),
                       ('model-metadata.json', objects), ('load-directives.json', directives),
                       ('static-model-candidates.json', candidates)]:
        (output / name).write_text(json.dumps(data, ensure_ascii=True, indent=2), encoding='utf-8')
    summary = [
        '# GTA SA installation inventory', '',
        'This is a local inspection result, not a working replacement engine.', '',
        f'Relevant files: {len(files)}', f'VER2 archives: {len(archives)}',
        f'IDE model definitions: {len(objects)}', f'Static DFF/TXD candidates: {len(candidates)}',
        f'Inspection errors: {len(errors)}', '',
        'Original files were opened for reading only. No assets were extracted.',
        'The report contains relative paths and asset names, not a proof of ownership.', '',
        '## Next milestone', '',
        'Choose one static candidate, privately read its DFF/TXD from the owner installation,',
        'implement bounded mesh/texture loaders, and render without gta_sa.exe.',
        'Grove Street and camera navigation follow successful model rendering.', '',
        '## Inspection errors', '',
    ] + [f"- {e['path']}: {e['error']}" for e in errors]
    (output / 'REPORT.md').write_text('\n'.join(summary) + '\n', encoding='utf-8')
    archive_path = output / 'sa-installation-report.zip'
    with zipfile.ZipFile(archive_path, 'w', zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(output.iterdir()):
            if path.suffix in ('.json', '.md'):
                archive.write(path, path.name)
    return archive_path


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game-dir', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        archive = inspect(args.game_dir, args.output)
    except (OSError, ValueError) as exc:
        print(f'Inspection failed: {exc}', file=sys.stderr)
        return 1
    print(f'Report: {archive}')
    print('No original assets were exported. Upload this report to continue.')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
