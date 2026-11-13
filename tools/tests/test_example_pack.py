import importlib.util
import json
import struct
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location(
    'make_example_pack', Path(__file__).parents[1] / 'make-example-pack.py')
make_example_pack = importlib.util.module_from_spec(spec)
spec.loader.exec_module(make_example_pack)

REPO = Path(__file__).parents[2]


class ExamplePackTests(unittest.TestCase):
    def test_generated_world_is_a_valid_glb_with_collision_roles(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'world.glb'
            make_example_pack.write_glb(make_example_pack.build_meshes(),
                                        make_example_pack.ground_texture(), path)
            data = path.read_bytes()
            magic, version, length = struct.unpack_from('<4sII', data)
            self.assertEqual((magic, version, length), (b'glTF', 2, len(data)))
            json_length, kind = struct.unpack_from('<I4s', data, 12)
            self.assertEqual(kind, b'JSON')
            document = json.loads(data[20:20 + json_length])
            names = [node['name'] for node in document['nodes']]
            self.assertIn('Ground', names)
            self.assertTrue(any(n.startswith('COL_') for n in names))
            self.assertTrue(any(n.startswith('NOCOL_') for n in names))
            self.assertEqual(document['images'][0]['mimeType'], 'image/png')

    def test_committed_pack_matches_the_generator(self):
        committed = REPO / 'content/example-town/world.glb'
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / 'world.glb'
            make_example_pack.write_glb(make_example_pack.build_meshes(),
                                        make_example_pack.ground_texture(), path)
            self.assertEqual(path.read_bytes(), committed.read_bytes(),
                             'run python tools/make-example-pack.py and commit the result')
        manifest = json.loads((REPO / 'content/example-town/content.json').read_text())
        self.assertEqual(manifest['license'], 'CC0-1.0')
        self.assertEqual(manifest['world']['scene'], 'world.glb')


if __name__ == '__main__':
    unittest.main()
