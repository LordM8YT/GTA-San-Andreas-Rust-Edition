"""Checks the committed SARE RP Town pack and server data against the
Blender generators (tools/blender/make_rp_town.py, make_rp_characters.py and
make_rp_vehicles.py) without running Blender."""
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


CHARACTERS = REPO / 'tools/blender/make_rp_characters.py'
VEHICLES = REPO / 'tools/blender/make_rp_vehicles.py'


def glb_document(path):
    data = path.read_bytes()
    magic, version, length = struct.unpack_from('<4sII', data)
    assert (magic, version, length) == (b'glTF', 2, len(data)), path
    json_length, kind = struct.unpack_from('<I4s', data, 12)
    assert kind == b'JSON', path
    return json.loads(data[20:20 + json_length]), len(data)


def lua_list(text, name):
    """The quoted strings of `name = { 'a', 'b' }` in a Lua table."""
    match = re.search(r'\b' + name + r' = \{([^}]*)\}', text)
    return re.findall(r"'([^']*)'", match.group(1))


def generator_constant(name, script=SCRIPT):
    tree = ast.parse(script.read_text(encoding='utf-8'))
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


    def test_characters_have_both_bodies_every_style_and_the_clips(self):
        manifest = json.loads((PACK / 'content.json').read_text(encoding='utf-8'))
        document, size = glb_document(PACK / manifest['characters'])
        self.assertLess(size, 2_000_000)
        names = [n.get('name', '') for n in document['nodes'] if 'mesh' in n]
        config = (RP / 'sare_rp_core/shared/config.lua').read_text(encoding='utf-8')
        for slot, constant in (('hair', 'HAIR'), ('top', 'TOPS'), ('pants', 'PANTS'), ('shoes', 'SHOES')):
            styles = generator_constant(constant, CHARACTERS)
            # The RP menus name the same styles in the same order.
            self.assertEqual(lua_list(config, slot), styles, slot)
            for body in 'MF':
                found = {int(n.split('.')[2]) for n in names if n.startswith(f'{body}.{slot}.')}
                self.assertEqual(found, set(range(len(styles))), f'{body} {slot}')
        self.assertTrue(any(n.startswith('M.base.0.') for n in names))
        self.assertTrue(any(n.startswith('F.base.0.') for n in names))
        clips = sorted(a['name'] for a in document['animations'])
        self.assertEqual(clips, sorted(generator_constant('CLIPS', CHARACTERS)))
        materials = {m['name'] for m in document['materials']}
        self.assertLessEqual({'skin', 'hair', 'top', 'pants', 'shoes'}, materials)

    def test_vehicles_have_four_wheels_paint_and_match_the_rp_config(self):
        manifest = json.loads((PACK / 'content.json').read_text(encoding='utf-8'))
        models = [v['model'] for v in manifest['vehicles']]
        self.assertEqual(models, ['sedan', 'compact', 'suv', 'pickup', 'sports', 'police', 'taxi',
                                  'ambulance', 'towtruck'])
        for vehicle in manifest['vehicles']:
            document, size = glb_document(PACK / vehicle['scene'])
            self.assertLess(size, 400_000, vehicle['model'])
            names = {n.get('name', '') for n in document['nodes']}
            self.assertLessEqual({'wheel_lf', 'wheel_rf', 'wheel_lb', 'wheel_rb'}, names, vehicle['model'])
            materials = {m['name']: m for m in document['materials']}
            self.assertIn('paint', materials)
            self.assertEqual(materials['glass'].get('alphaMode'), 'BLEND')
            self.assertEqual(len(vehicle['color']), 3)
            self.assertEqual(len(vehicle['seat']), 3)
        config = (RP / 'sare_rp_core/shared/config.lua').read_text(encoding='utf-8')
        dealer = re.findall(r"\{ model = '(\w+)', label", config.split('RPConfig.Dealer = {', 1)[1].split('\n}', 1)[0])
        jobs = re.findall(r"= '(\w+)'", config.split('RPConfig.JobVehicle = {', 1)[1].split('}', 1)[0])
        self.assertTrue(dealer and jobs)
        self.assertLessEqual(set(dealer) | set(jobs), set(models))


if __name__ == '__main__':
    unittest.main()
