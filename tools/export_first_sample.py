"""Privately copy only the first DFF/TXD test pair from the owner's local install."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import sys
import zipfile

from inspect_installation import img_index, safe_output, SECTOR

NAMES = {'cj_wastebin.dff': 0x10, 'cj_bins.txd': 0x16}


def export(game: Path, output: Path) -> Path:
    game, output = safe_output(game, output)
    source = game / 'models/gta3.img'
    if source.is_symlink() or not source.resolve().is_relative_to(game):
        raise ValueError('Archive must be inside the installation and not a symlink')
    index = img_index(source)
    selected = {}
    for entry in index['entries']:
        name = entry['name'].lower()
        if name in NAMES:
            if name in selected:
                raise ValueError(f'Duplicate archive entry: {name}')
            selected[name] = entry
    if set(selected) != set(NAMES):
        raise ValueError('The expected cj_wastebin.dff / cj_bins.txd pair was not found')
    payloads, metadata = {}, []
    with source.open('rb') as stream:
        for name, entry in selected.items():
            if not entry['classic_range_valid'] or entry['archive_sectors']:
                raise ValueError(f'Unsupported or invalid classic entry: {name}')
            size = entry['streaming_sectors'] * SECTOR
            if size > 16 * 1024 * 1024:
                raise ValueError(f'Test entry exceeds 16 MiB: {name}')
            stream.seek(entry['sector_offset'] * SECTOR)
            data = stream.read(size)
            if len(data) != size or len(data) < 12:
                raise ValueError(f'Truncated entry: {name}')
            chunk_type, chunk_size, version = struct.unpack_from('<III', data)
            if chunk_type != NAMES[name] or chunk_size > len(data) - 12:
                raise ValueError(f'Unexpected RenderWare root chunk: {name}')
            payloads[name] = data
            metadata.append({'name': name, 'source': 'models/gta3.img',
                             'sector_offset': entry['sector_offset'], 'bytes': len(data),
                             'root_chunk_type': chunk_type, 'root_chunk_bytes': chunk_size,
                             'rw_library_id': version, 'sha256': hashlib.sha256(data).hexdigest()})
    output.mkdir(parents=True, exist_ok=False)
    for name, data in payloads.items():
        (output / name).write_bytes(data)
    manifest = {'model': 'CJ_WASTEBIN', 'model_id': 1347, 'txd': 'CJ_BINS',
                'purpose': 'Private local test sample; do not distribute with runtime source or binaries',
                'source_installation_changed': False, 'files': metadata}
    (output / 'sample.json').write_text(json.dumps(manifest, indent=2), encoding='utf-8')
    result = output / 'sa-first-model-private.zip'
    with zipfile.ZipFile(result, 'w', zipfile.ZIP_DEFLATED) as archive:
        for name in [*payloads, 'sample.json']:
            archive.write(output / name, name)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--game-dir', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        result = export(args.game_dir, args.output)
    except (ValueError, OSError) as exc:
        print(f'Export failed: {exc}', file=sys.stderr)
        return 1
    print(f'Private test sample: {result}')
    print('This ZIP contains two original assets for private testing, not public distribution.')
    print('Original installation unchanged. Attach the private ZIP to continue loader testing.')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
