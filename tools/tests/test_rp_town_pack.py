"""Checks the committed SARE RP Town pack and server data against the
Blender generator (tools/blender/make_rp_town.py) without running Blender."""
import ast
import json
import re
import struct
import unittest
from pathlib import Path

REPO = Path(__file__).parents[2]
SCRIPT = REPO / 'tools/blender/make_rp_town.py'
PACK = REPO / 'content/sare-rp-town'
RP = REPO / 'server-data/resources/[sare-rp]'


def generator_constant(name):
    tree = ast.parse(SCRIPT.read_text(encoding='utf-8'))
    for node in tree.body:
        if isinstance(node, ast.Assign) and any(getattr(t, 'id', None) == name for t in node.targets):
            return ast.literal_eval(node.value)
    raise KeyError(name)


def lua_points(text):
    """name -> list of (x, y, z, heading) from locations.lua."""
    points, current = {}, None
    for line in text.splitlines():
        start = re.match(r'\s{2}(\w+) = \{(.*)', line)
        if start:
            current = start.group(1)
            points[current] = []
        for match in re.finditer(r'x = (-?[\d.]+), y = (-?[\d.]+), z = (-?[\d.]+), heading = (-?[\d.]+)', line):
            points[current].append(tuple(float(v) for v in match.groups()))
    return points


class RpTownPackTests(unittest.TestCase):
    def test_pack_is_a_valid_cc0_glb_with_collision_roles(self):
        manifest = json.loads((PACK / 'content.json').read_text(encoding='utf-8'))
        self.assertEqual(manifest['id'], 'sare-rp-town')
        self.assertEqual(manifest['license'], 'CC0-1.0')
        data = (PACK / manifest['world']['scene']).read_bytes()
        magic, version, length = struct.unpack_from('<4sII', data)
        self.assertEqual((magic, version, length), (b'glTF', 2, len(data)))
        json_length, kind = struct.unpack_from('<I4s', data, 12)
        self.assertEqual(kind, b'JSON')
        document = json.loads(data[20:20 + json_length])
        names = [node.get('name', '') for node in document['nodes']]
        self.assertTrue(any(n.startswith('COL_') for n in names), 'an invisible edge wall')
        self.assertTrue(any(n.startswith('NOCOL_Sign') for n in names), 'signs you can walk through')
        self.assertTrue(all(i['mimeType'] == 'image/png' for i in document.get('images', [])))
        self.assertLess(len(data), 4_000_000, 'keep the pack small')

    def test_locations_lua_matches_the_generator(self):
        anchor = generator_constant('ANCHOR')
        expected = generator_constant('LOCATIONS')
        points = lua_points((RP / 'sare_rp_core/shared/locations.lua').read_text(encoding='utf-8'))
        self.assertEqual(set(points), set(expected))
        for name, value in expected.items():
            values = value if isinstance(value, list) else [value]
            self.assertEqual(len(points[name]), len(values), name)
            for (x, y, z, h), got in zip(values, points[name]):
                self.assertAlmostEqual(got[0], anchor[0] + x, places=2, msg=name)
                self.assertAlmostEqual(got[1], anchor[1] + y, places=2, msg=name)
                self.assertAlmostEqual(got[3], h, places=1, msg=name)
        spawns = re.findall(r"x = (-?[\d.]+), y = (-?[\d.]+)",
                            (RP / 'sare-rp-map/map.lua').read_text(encoding='utf-8'))
        self.assertEqual([(float(x), float(y)) for x, y in spawns],
                         [(anchor[0] + p[0], anchor[1] + p[1]) for p in expected['spawns']])

    def test_every_configured_place_has_a_location(self):
        config = (RP / 'sare_rp_core/shared/config.lua').read_text(encoding='utf-8')
        block = config.split('RPConfig.Places = {', 1)[1].split('\n}', 1)[0]
        places = re.findall(r'^\s{2}(\w+) = \{', block, re.M)
        self.assertGreater(len(places), 10)
        self.assertLessEqual(set(places), set(generator_constant('LOCATIONS')))


if __name__ == '__main__':
    unittest.main()
