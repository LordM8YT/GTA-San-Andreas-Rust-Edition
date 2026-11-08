"""Tests for the bounded GTA San Andreas -> SARE resource importer."""
import importlib.util
import json
from pathlib import Path
import struct
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('import_gta_sa', ROOT / 'import-gta-sa.py')
IMPORTER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(IMPORTER)


def write_img(path, entries):
    """Create a small VER2 IMG archive with sector-aligned, uncompressed entries."""
    directory_bytes = len(entries) * 32
    first_sector = (8 + directory_bytes + IMPORTER.IMG_SECTOR - 1) // IMPORTER.IMG_SECTOR
    rows = []
    payload = bytearray()
    sector = first_sector
    for name, content in entries:
        encoded = name.encode('ascii')
        if len(encoded) > 23:
            raise ValueError(name)
        padded = content + bytes((-len(content)) % IMPORTER.IMG_SECTOR)
        sectors = len(padded) // IMPORTER.IMG_SECTOR
        rows.append(struct.pack('<IHH24s', sector, sectors, 0, encoded))
        payload.extend(bytes((sector - first_sector) * IMPORTER.IMG_SECTOR - len(payload)))
        payload.extend(padded)
        sector += sectors
    path.write_bytes(b'VER2' + struct.pack('<I', len(entries)) + b''.join(rows)
                     + bytes(first_sector * IMPORTER.IMG_SECTOR - 8 - directory_bytes)
                     + payload)


class GtaSaImportTests(unittest.TestCase):
    def test_imports_classic_img_ide_ipl_as_native_and_fivem_style_resource(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            source = temp / 'old_mod'
            source.mkdir()
            (source / 'objects.ide').write_text(
                'objs\n100, road, roads, 100, 0\nend\n', encoding='utf-8')
            (source / 'roads.ipl').write_text(
                'inst\n100, road, 0, 10, 20, 30, 0, 0, 0, 1, -1\nend\n', encoding='utf-8')
            write_img(source / 'gta3.img', [
                ('road.dff', b'owned-dff-fixture'),
                ('roads.txd', b'owned-txd-fixture'),
                ('road.col', b'owned-col-fixture'),
            ])
            out = temp / 'mods' / '[maps]' / 'old_mod'
            report = IMPORTER.import_resource(source, out, kind='map')
            manifest = json.loads((out / 'resource.json').read_text())
            self.assertFalse(manifest['enabled'])
            self.assertEqual(manifest['models'][0]['id'], 100)
            self.assertEqual(manifest['placements'][0]['model_id'], 100)
            self.assertEqual(manifest['placements'][0]['position'], [10.0, 20.0, 30.0])
            self.assertIn('fx_version', (out / 'fxmanifest.lua').read_text())
            self.assertIn("game 'gta3'", (out / 'fxmanifest.lua').read_text())
            self.assertTrue((out / manifest['models'][0]['dff']).read_bytes().startswith(b'owned-dff-fixture'))
            self.assertIn('road', report['models'])
            self.assertEqual(report['compressed_img_entries'], 0)

    def test_vehicle_pack_uses_vehicles_ide_and_shared_img_assets(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            source = temp / 'vehicle_pack'
            source.mkdir()
            (source / 'vehicles.ide').write_text(
                'cars\n411, elegy, elegy, car, ELEGY, ELEGY, null\nend\n', encoding='utf-8')
            write_img(source / 'vehicles.img', [
                ('elegy.dff', b'elegy-model'),
                ('elegy.txd', b'elegy-textures'),
            ])
            out = temp / 'mods' / '[vehicles]' / 'elegy'
            result = IMPORTER.import_resource(source, out, kind='auto')
            manifest = json.loads((out / 'resource.json').read_text())
            self.assertEqual(result['kind'], 'vehicles')
            self.assertEqual(manifest['vehicles'][0]['name'], 'elegy')
            self.assertTrue((out / manifest['vehicles'][0]['txd']).read_bytes().startswith(b'elegy-textures'))

    def test_game_dir_reads_only_registered_ide_files_and_retains_vanilla_ipl_placements(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            source = temp / 'map_pack'
            game = temp / 'game'
            source.mkdir()
            (game / 'data').mkdir(parents=True)
            (game / 'models').mkdir()
            # A normal game archive is deliberately larger than the importer budget;
            # --game-dir must not recursively index or read it.
            (game / 'models' / 'gta3.img').write_bytes(bytes(IMPORTER.MAX_INPUT_BYTES + 1))
            (game / 'data' / 'objects.ide').write_text(
                'objs\n1, vanilla, generic, 100, 0\nend\n', encoding='utf-8')
            (game / 'data' / 'unused.ide').write_text(
                'objs\n2, not_registered, generic, 100, 0\nend\n', encoding='utf-8')
            (game / 'data' / 'default.dat').write_text(
                'IDE data/objects.ide\n', encoding='utf-8')
            (game / 'data' / 'gta.dat').write_text('', encoding='utf-8')
            (source / 'map.ide').write_text(
                'objs\n300, prop, props, 100, 0\nend\n', encoding='utf-8')
            (source / 'map.ipl').write_text(
                'inst\n1, vanilla, 0, 1, 2, 3, 0, 0, 0, 1, -1\n'
                '2, not_registered, 0, 7, 8, 9, 0, 0, 0, 1, -1\n'
                '300, prop, 0, 4, 5, 6, 0, 0, 0, 1, -1\nend\n', encoding='utf-8')
            write_img(source / 'map.img', [('prop.dff', b'prop-dff')])
            out = temp / 'native_map'
            IMPORTER.import_resource(source, out, kind='map', game_dir=game)
            manifest = json.loads((out / 'resource.json').read_text())
            self.assertEqual([p['model_id'] for p in manifest['placements']], [1, 300])

    def test_skips_interiors_and_binary_ipls_without_corrupting_exterior_placements(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            source = temp / 'map_pack'
            source.mkdir()
            (source / 'map.ide').write_text('objs\n300, prop, props, 100, 0\nend\n', encoding='utf-8')
            (source / 'map.ipl').write_text(
                'inst\n300, prop, 0, 1, 2, 3, 0, 0, 0, 1, -1\n'
                '300, prop, 1, 4, 5, 6, 0, 0, 0, 1, -1\nend\n', encoding='utf-8')
            (source / 'binary.ipl').write_bytes(b'bnry' + bytes(64))
            write_img(source / 'map.img', [('prop.dff', b'prop-dff')])
            out = temp / 'native_map'
            report = IMPORTER.import_resource(source, out, kind='map')
            manifest = json.loads((out / 'resource.json').read_text())
            self.assertEqual(len(manifest['placements']), 1)
            self.assertEqual(report['skipped_interior_ipl_placements'], 1)
            self.assertEqual(report['binary_ipl_files'], ['binary.ipl'])

    def test_props_require_position_and_start_disabled(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            source = temp / 'props'
            source.mkdir()
            (source / 'crate.dff').write_bytes(b'crate')
            with self.assertRaisesRegex(ValueError, 'Props require'):
                IMPORTER.import_resource(source, temp / 'bad', kind='props')
            IMPORTER.import_resource(source, temp / 'good', kind='props', position=[10.0, 20.0, 3.0])
            manifest = json.loads((temp / 'good' / 'resource.json').read_text())
            self.assertFalse(manifest['enabled'])
            self.assertEqual(manifest['placements'][0]['position'], [10.0, 20.0, 3.0])

    def test_rejects_bad_img_and_does_not_overwrite_output(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            source = temp / 'broken'
            source.mkdir()
            (source / 'bad.img').write_bytes(b'NOPE' + bytes(32))
            with self.assertRaisesRegex(ValueError, 'VER2 IMG'):
                IMPORTER.import_resource(source, temp / 'out')
            existing = temp / 'existing'
            existing.mkdir()
            with self.assertRaisesRegex(ValueError, 'already exists'):
                IMPORTER.import_resource(source, existing)

    def test_rejects_symlinked_inputs(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            source = temp / 'source'
            outside = temp / 'outside'
            source.mkdir()
            outside.mkdir()
            (outside / 'external.dff').write_bytes(b'external')
            try:
                (source / 'escape').symlink_to(outside, target_is_directory=True)
            except OSError:
                self.skipTest('Symlinks are unavailable on this host')
            with self.assertRaisesRegex(ValueError, 'Links and junctions'):
                IMPORTER.checked_tree(source)


if __name__ == '__main__':
    unittest.main()
