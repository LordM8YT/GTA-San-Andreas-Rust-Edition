import hashlib
import json
from pathlib import Path
import struct
import sys
import tempfile
import unittest
import zipfile

sys.path.insert(0, str(Path(__file__).parents[1] / 'tools'))
from export_first_sample import export


class ExportTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.base = Path(self.tmp.name)
        self.game = self.base / 'game'
        (self.game / 'models').mkdir(parents=True)
        self.source = self.game / 'models/gta3.img'

    def tearDown(self):
        self.tmp.cleanup()

    def fixture(self, wrong_root=False):
        names = ['cj_wastebin.dff', 'cj_bins.txd']
        header = b'VER2' + struct.pack('<I', 2)
        for i, name in enumerate(names):
            header += struct.pack('<IHH24s', i+1, 1, 0, name.encode())
        data = header.ljust(2048, b'\0')
        for root in ([99, 0x16] if wrong_root else [0x10, 0x16]):
            data += (struct.pack('<III', root, 4, 0x1803FFFF) + b'test').ljust(2048, b'\0')
        self.source.write_bytes(data)

    def test_private_pair_only_and_original_unchanged(self):
        self.fixture()
        before = hashlib.sha256(self.source.read_bytes()).hexdigest()
        result = export(self.game, self.base / 'sample')
        self.assertEqual(before, hashlib.sha256(self.source.read_bytes()).hexdigest())
        with zipfile.ZipFile(result) as archive:
            self.assertEqual(set(archive.namelist()), {'cj_wastebin.dff', 'cj_bins.txd', 'sample.json'})
            manifest = json.loads(archive.read('sample.json'))
            for entry in manifest['files']:
                self.assertEqual(entry['sha256'], hashlib.sha256(archive.read(entry['name'])).hexdigest())

    def test_wrong_renderware_root_rejected_before_output(self):
        self.fixture(wrong_root=True)
        output = self.base / 'sample'
        with self.assertRaises(ValueError):
            export(self.game, output)
        self.assertFalse(output.exists())

    def test_output_in_game_rejected(self):
        self.fixture()
        with self.assertRaises(ValueError):
            export(self.game, self.game / 'sample')


if __name__ == '__main__':
    unittest.main()
