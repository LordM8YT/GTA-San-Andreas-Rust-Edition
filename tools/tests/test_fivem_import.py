"""Resource-folder import checks with owned XML meshes and inert Lua text."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('import_fivem', ROOT / 'tools/import-fivem.py')
f = importlib.util.module_from_spec(spec)
spec.loader.exec_module(f)


def resource(root):
    (root / 'stream').mkdir(parents=True)
    (root / 'fxmanifest.lua').write_text("fx_version 'cerulean'\ngame 'gta5'\nclient_script 'client.lua'\nui_page 'ui/index.html'\nos.execute('this must never execute')\n", encoding='utf-8')
    (root / 'client.lua').write_text('error("do not run")', encoding='utf-8')
    return root


def mesh(path, vehicle=False):
    xml = '''<Drawable><ShaderGroup><Shaders><Item/></Shaders></ShaderGroup>
    <DrawableModelsHigh><Item><Geometries><Item><ShaderIndex value="0"/>
    <VertexBuffer><Layout><Position/><Normal/></Layout><Data>
    -0.7 -1.5 0  0 0 1
    0.7 -1.5 0  0 0 1
    0 1.5 1  0 0 1
    </Data></VertexBuffer><IndexBuffer><Data>0 1 2</Data></IndexBuffer>
    </Item></Geometries></Item></DrawableModelsHigh></Drawable>'''
    if vehicle:
        xml = '<Fragment>' + xml + '</Fragment>'
    path.write_text(xml, encoding='utf-8')
    return path


def map_xml(path, archetype='prop', rotation='0 0 0 1', scale='1', kind='CEntityDef', lod='LODTYPES_DEPTH_HD'):
    x, y, z, w = rotation.split()
    path.write_text(f'''<CMapData><entities><Item type="{kind}">
    <archetypeName>{archetype}</archetypeName><position x="10" y="20" z="3"/>
    <rotation x="{x}" y="{y}" z="{z}" w="{w}"/>
    <scaleXY value="{scale}"/><scaleZ value="1"/><lodLevel>{lod}</lodLevel>
    </Item></entities></CMapData>''', encoding='utf-8')
    return path


class FiveMImportTests(unittest.TestCase):
    def test_static_map_preserves_inverse_quaternion_and_translates_positions(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp); root = resource(temp / 'input')
            mesh(root / 'stream/prop.ydr.xml')
            rotation = '0 0 0.70710678 0.70710678'
            map_xml(root / 'stream/map.ymap.xml', rotation=rotation)
            report = f.import_resource(root, temp / 'map', 'map', ymap='stream/map.ymap.xml', offset=[100., -20., 5.])
            manifest = json.loads((temp / 'map/resource.json').read_text())
            placement = manifest['placements'][0]
            self.assertEqual(placement['position'], [110., 0., 8.])
            self.assertAlmostEqual(placement['rotation'][2], 2**-.5)
            self.assertAlmostEqual(placement['rotation'][3], 2**-.5)
            # The native IPL convention inverts this quaternion. A +X vertex
            # therefore turns toward -Y, matching CodeWalker's CEntityDef.
            x, y, z, w = placement['rotation']
            self.assertAlmostEqual(1 - 2*(y*y + z*z), 0., places=6)
            self.assertAlmostEqual(2*(x*y - z*w), -1., places=6)
            self.assertEqual(report['map']['placements'], 1)
            self.assertEqual(report['map']['skipped_lods'], [])

    def test_map_hash_resolution_and_skipped_low_detail_entities(self):
        # Independent reference: Cfx vehicle-model list gives Adder 3078201489.
        self.assertEqual(f.map_converter.jenkins('adder'), 3078201489)
        self.assertEqual(f.map_converter.jenkins('ADDER'), 3078201489)
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp); root = resource(temp / 'input')
            mesh(root / 'stream/prop.ydr.xml')
            path = map_xml(root / 'stream/map.ymap.xml', archetype=f'hash_{f.map_converter.jenkins("prop"):08X}')
            tree = f.converter.parse_xml(path)
            lod = f.converter.ET.SubElement(tree.find('entities'), 'Item', type='CEntityDef')
            f.converter.ET.SubElement(lod, 'lodLevel').text = 'LODTYPES_DEPTH_LOD'
            f.converter.ET.ElementTree(tree).write(path)
            report = f.import_resource(root, temp / 'map', 'map', ymap='stream/map.ymap.xml')
            self.assertEqual(report['map']['placements'], 1)
            self.assertEqual(report['map']['skipped_lods'], [dict(entity=1, reason='LODTYPES_DEPTH_LOD')])

    def test_map_missing_assets_mlo_scale_and_malformed_rotation_fail_atomically(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp); root = resource(temp / 'input')
            mesh(root / 'stream/prop.ydr.xml')
            path = root / 'stream/map.ymap.xml'
            for index, (options, error) in enumerate([
                (dict(archetype='missing_prop'), 'Missing YDR'),
                (dict(kind='CMloInstanceDef'), 'MLO instances'),
                (dict(scale='2'), 'unsupported scale'),
                (dict(rotation='0 0 0 0'), 'rotation quaternion'),
                (dict(rotation='nan 0 0 1'), 'Invalid entity rotation'),
            ]):
                map_xml(path, **options)
                output = temp / f'failed-{index}'
                with self.assertRaisesRegex(ValueError, error):
                    f.import_resource(root, output, 'map', ymap='stream/map.ymap.xml')
                self.assertFalse(output.exists())

    def test_inspection_reports_scripts_mlo_and_peds_without_execution(self):
        with tempfile.TemporaryDirectory() as temp:
            root = resource(Path(temp) / 'input')
            for name in ('room.ymap', 'room.ytyp', 'ped.ydd'):
                (root / 'stream' / name).write_bytes(b'inert')
            _, report = f.inspect_resource(root)
            self.assertEqual(report['scripts'], ['client.lua'])
            self.assertEqual(len(report['assets']), 3)
            self.assertTrue(any('MLO' in text for text in report['warnings']))
            self.assertTrue(any('NUI' in text for text in report['warnings']))
            self.assertTrue(any('target rig' in text for text in report['warnings']))

    def test_two_car_pack_merges_assets_and_skips_hi_variant(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp); root = resource(temp / 'input')
            for name in ('first.yft.xml', 'first_hi.yft.xml', 'second.yft.xml'):
                mesh(root / 'stream' / name, vehicle=True)
            output = temp / 'native'
            report = f.import_resource(root, output, 'vehicles', enable=True)
            manifest = json.loads((output / 'resource.json').read_text())
            self.assertEqual(len(manifest['vehicles']), 2)
            self.assertTrue(manifest['enabled'])
            self.assertEqual([item['source'] for item in report['converted']],
                             ['stream/first.yft.xml', 'stream/second.yft.xml'])
            self.assertFalse(list(output.rglob('*.lua')))
            for vehicle in manifest['vehicles']:
                self.assertTrue((output / vehicle['dff']).is_file())
            with self.assertRaisesRegex(ValueError, 'Output already exists'):
                f.import_resource(root, output, 'vehicles')
            audit = ROOT / 'native/target/debug/examples' / ('audit_resource.exe' if os.name == 'nt' else 'audit_resource')
            if audit.exists():
                for vehicle in manifest['vehicles']:
                    result = subprocess.run([str(audit), str(output / vehicle['dff'])], capture_output=True, text=True)
                    self.assertEqual(result.returncode, 0, result.stderr)

    def test_explicit_hi_selection_and_prop_preview_placements(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp); root = resource(temp / 'input')
            for name in ('one.ydr.xml', 'two.ydr.xml'):
                mesh(root / 'stream' / name)
            with self.assertRaisesRegex(ValueError, 'Props require'):
                f.import_resource(root, temp / 'no-position', 'props')
            f.import_resource(root, temp / 'props', 'props', position=[10., 20., 3.], model_id=30100)
            manifest = json.loads((temp / 'props/resource.json').read_text())
            self.assertFalse(manifest['enabled'])
            self.assertEqual(manifest['placements'], [dict(model_id=30100, position=[10., 20., 3.]),
                                                     dict(model_id=30101, position=[13., 20., 3.])])
            mesh(root / 'stream/one.yft.xml', True); mesh(root / 'stream/one_hi.yft.xml', True)
            report = f.import_resource(root, temp / 'hi', 'vehicles', ['stream/one_hi.yft.xml'])
            self.assertEqual(report['converted'][0]['source'], 'stream/one_hi.yft.xml')

    def test_failed_second_conversion_never_publishes_partial_pack(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp); root = resource(temp / 'input')
            mesh(root / 'stream/first.yft.xml', True)
            (root / 'stream/second.yft.xml').write_text('<Fragment/>')
            with self.assertRaises(ValueError):
                f.import_resource(root, temp / 'native', 'vehicles')
            self.assertFalse((temp / 'native').exists())
            self.assertEqual(list(temp.glob('.sa-fivem-*')), [])
            self.assertTrue((root / 'stream/first.yft.xml').exists())

    def test_missing_manifest_wrong_kind_and_source_output_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp); root = temp / 'input'; root.mkdir()
            with self.assertRaisesRegex(ValueError, 'No fxmanifest'):
                f.inspect_resource(root)
            resource(root); mesh(root / 'stream/prop.ydr.xml')
            with self.assertRaisesRegex(ValueError, 'No compatible'):
                f.import_resource(root, temp / 'cars', 'vehicles')
            with self.assertRaisesRegex(ValueError, 'outside the source'):
                f.import_resource(root, root / 'native', 'vehicles')
            with self.assertRaisesRegex(ValueError, 'Requested model missing'):
                f.import_resource(root, temp / 'props', 'props', ['../outside.ydr.xml'], [0., 0., 0.])

    def test_ambiguous_texture_sources_do_not_publish(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp); root = resource(temp / 'input')
            mesh(root / 'stream/car.yft.xml', True)
            for folder in ('a', 'b'):
                (root / folder).mkdir(); (root / folder / 'car.ytd.xml').write_text('<TextureDictionary/>')
            with self.assertRaisesRegex(ValueError, 'Ambiguous texture'):
                f.import_resource(root, temp / 'native', 'vehicles')
            self.assertFalse((temp / 'native').exists())

    def test_symlink_rejected_before_reading_external_files(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp); root = resource(temp / 'input')
            outside = temp / 'outside'; outside.mkdir(); (outside / 'secret').write_text('private')
            try:
                (root / 'escape').symlink_to(outside, target_is_directory=True)
            except OSError:
                self.skipTest('Creating symlinks is unavailable on this host')
            with self.assertRaisesRegex(ValueError, 'Links/junctions'):
                f.inspect_resource(root)

    @unittest.skipUnless(os.name == 'nt', 'Windows junction check')
    def test_windows_junction_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp); root = resource(temp / 'input')
            outside = temp / 'outside'; outside.mkdir()
            (outside / 'secret').write_text('private')
            junction = root / 'escape'
            result = subprocess.run(['cmd', '/c', 'mklink', '/J', str(junction), str(outside)],
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            try:
                with self.assertRaisesRegex(ValueError, 'Links/junctions'):
                    f.inspect_resource(root)
                self.assertEqual((outside / 'secret').read_text(), 'private')
            finally:
                os.rmdir(junction)


if __name__ == '__main__':
    unittest.main()
