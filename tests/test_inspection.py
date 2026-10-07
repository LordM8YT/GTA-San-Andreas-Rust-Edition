import hashlib
import importlib.util
import json
import os
from pathlib import Path
import struct
import tempfile
import unittest
import zipfile

spec = importlib.util.spec_from_file_location('audit', Path(__file__).parents[1] / 'tools/inspect_installation.py')
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


def fixture_img(path, entries):
    directory = b'VER2' + struct.pack('<I', len(entries))
    for i, name in enumerate(entries):
        directory += struct.pack('<IHH24s', i + 1, 1, 0, name.encode('ascii'))
    path.write_bytes(directory.ljust(2048, b'\0') + b'fixture-only'.ljust(2048, b'\0') * len(entries))


class AuditTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.base = Path(self.tmp.name)
        self.game = self.base / 'game'
        (self.game / 'models').mkdir(parents=True)
        (self.game / 'data/maps').mkdir(parents=True)

    def tearDown(self):
        self.tmp.cleanup()

    def test_index_and_report_without_source_changes_or_asset_export(self):
        fixture_img(self.game / 'models/gta3.img', ['example.dff', 'example.txd'])
        (self.game / 'data/gta.dat').write_text('IMG models/gta3.img\nIDE data/maps/sample.ide\nIPL data/maps/sample.ipl\n')
        (self.game / 'data/maps/sample.ide').write_text('objs\n42, example, example, 1, 100.0, 0\nend\n')
        before = {p.relative_to(self.game): hashlib.sha256(p.read_bytes()).hexdigest()
                  for p in self.game.rglob('*') if p.is_file()}
        result = audit.inspect(self.game, self.base / 'out')
        after = {p.relative_to(self.game): hashlib.sha256(p.read_bytes()).hexdigest()
                 for p in self.game.rglob('*') if p.is_file()}
        self.assertEqual(before, after)
        with zipfile.ZipFile(result) as archive:
            self.assertEqual(len(archive.namelist()), 6)
            self.assertTrue(all(Path(n).suffix in ('.json', '.md') for n in archive.namelist()))
            candidates = json.loads(archive.read('static-model-candidates.json'))
            self.assertEqual(candidates[0]['model'], 'example')
            self.assertEqual(candidates[0]['dff_locations'], ['models/gta3.img'])
            index = json.loads(archive.read('img-indexes.json'))[0]
            self.assertEqual(index['invalid_ranges'], 0)
            self.assertEqual(index['entries'][1]['sector_offset'], 2)

    def test_reject_in_installation_output(self):
        with self.assertRaises(ValueError):
            audit.inspect(self.game, self.game / 'cache')
        self.assertFalse((self.game / 'cache').exists())

    def test_reject_ancestor_output_and_existing_output(self):
        for destination in (self.base, self.game):
            with self.assertRaises(ValueError):
                audit.inspect(self.game, destination)
        existing = self.base / 'existing'
        existing.mkdir()
        (existing / 'sentinel').write_text('keep')
        with self.assertRaises(ValueError):
            audit.inspect(self.game, existing)
        self.assertEqual((existing / 'sentinel').read_text(), 'keep')

    def test_reject_unsupported_and_truncated_directory(self):
        path = self.game / 'models/bad.img'
        for data in (b'', b'OTHERIMG', b'VER2' + struct.pack('<I', 2),
                     b'VER2' + struct.pack('<I', audit.MAX_ENTRIES + 1)):
            path.write_bytes(data)
            with self.assertRaises(ValueError):
                audit.img_index(path)

    def test_invalid_entry_range_is_reported(self):
        path = self.game / 'models/bad.img'
        path.write_bytes(b'VER2' + struct.pack('<I', 1) + struct.pack('<IHH24s', 9, 1, 0, b'bad.dff'))
        self.assertEqual(audit.img_index(path)['invalid_ranges'], 1)

    def test_bad_archive_is_recorded_and_good_inventory_continues(self):
        (self.game / 'models/bad.img').write_bytes(b'not-img')
        output = self.base / 'out'
        audit.inspect(self.game, output)
        report = json.loads((output / 'inventory.json').read_text())
        self.assertEqual(report['errors'][0]['path'], 'models/bad.img')
        self.assertFalse(report['ownership_verified'])

    def test_symlink_outside_installation_is_skipped(self):
        outside = self.base / 'outside.ide'
        outside.write_text('objs\n99, hidden, hidden, 1, 100, 0\nend\n')
        try:
            (self.game / 'data/link.ide').symlink_to(outside)
        except OSError as exc:
            if os.name == 'nt' and getattr(exc, 'winerror', None) == 1314:
                self.skipTest('Windows symlink privilege is unavailable')
            raise
        output = self.base / 'out'
        audit.inspect(self.game, output)
        self.assertEqual(json.loads((output / 'model-metadata.json').read_text()), [])


if __name__ == '__main__':
    unittest.main()
