"""Resource-folder import checks with owned XML meshes and inert Lua text."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from test_gta5_conversion import owned_clothing, AUDIT

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
    def rig(self, mapping, **extra):
        return dict(base_player=ROOT/'mods/native-ped-demo/ped.dff',
                    base_ifp=ROOT/'mods/native-ped-demo/ped.ifp', bone_map=mapping, **extra)

    def test_player_folder_import_selects_one_ydd_and_preserves_native_animation(self):
        with tempfile.TemporaryDirectory() as temp:
            temp=Path(temp); root=resource(temp/'input')
            source,mapping,_=owned_clothing(root/'stream', ROOT/'mods/native-ped-demo/ped.dff')
            source.rename(root/'stream/character.ydd.xml')
            output=temp/'player'
            report=f.import_resource(root,output,'player',enable=True,**self.rig(mapping))
            manifest=json.loads((output/'resource.json').read_text())
            self.assertEqual(manifest['name'],'character'); self.assertTrue(manifest['enabled'])
            self.assertNotIn('clothes',manifest['player'])
            self.assertEqual((output/manifest['player']['ifp']).read_bytes(),(ROOT/'mods/native-ped-demo/ped.ifp').read_bytes())
            self.assertFalse((output/'stream/rig/base.dff').exists())
            self.assertEqual(report['converted'][0]['source'],'stream/character.ydd.xml')
            self.assertFalse(any(path.suffix=='.lua' for path in output.rglob('*')))
            if AUDIT.exists():
                result=subprocess.run([str(AUDIT),str(output/manifest['player']['dff']),'--skin'],capture_output=True,text=True)
                self.assertEqual(result.returncode,0,result.stderr)

    def test_clothing_folder_import_combines_two_items_and_copies_base_rig_once(self):
        with tempfile.TemporaryDirectory() as temp:
            temp=Path(temp); root=resource(temp/'input')
            source,mapping,_=owned_clothing(root/'stream'); source.rename(root/'stream/shirt.ydd.xml')
            source,mapping,_=owned_clothing(root/'stream',ROOT/'mods/native-clothing-demo/hat.dff'); source.rename(root/'stream/cap.ydd.xml')
            output=temp/'clothes'; report=f.import_resource(root,output,'clothing',**self.rig(mapping))
            manifest=json.loads((output/'resource.json').read_text()); player=manifest['player']
            self.assertEqual([c['name'] for c in player['clothes']],['cap','shirt'])
            self.assertEqual(len(list(output.rglob('*.ifp'))),1)
            self.assertEqual((output/player['dff']).read_bytes(),(ROOT/'mods/native-ped-demo/ped.dff').read_bytes())
            self.assertEqual(set(report['target_rig']),{'base_player','base_ifp','bone_map'})
            for clothing in player['clothes']:
                self.assertEqual(f.converter.native_rig(output/clothing['dff'])[1:],f.converter.native_rig(output/player['dff'])[1:])

    @unittest.skipUnless((ROOT/'tools/tests/map-fixture/bin/Release/net9.0/MapFixture.dll').exists(), 'Build MapFixture')
    def test_owned_binary_ydd_roundtrip_retargets_a_complete_player(self):
        with tempfile.TemporaryDirectory() as temp:
            temp=Path(temp); root=resource(temp/'input')
            source,mapping,expected=owned_clothing(root/'stream',ROOT/'mods/native-ped-demo/ped.dff')
            drawable=f.converter.parse_xml(source); drawable.tag='Item'
            f.converter.ET.SubElement(drawable,'Name').text='owned_character'
            dictionary=f.converter.ET.Element('DrawableDictionary'); dictionary.append(drawable)
            f.converter.ET.ElementTree(dictionary).write(source)
            binary=root/'stream/character.ydd'
            result=subprocess.run(['dotnet',str(ROOT/'tools/tests/map-fixture/bin/Release/net9.0/MapFixture.dll'),str(source),str(binary)],capture_output=True,text=True)
            self.assertEqual(result.returncode,0,result.stderr); self.assertEqual(binary.read_bytes()[:4],b'RSC7')
            report=f.import_resource(root,temp/'native','player',requested=['stream/character.ydd'],**self.rig(mapping))
            self.assertGreater(report['converted'][0]['vertices'],0)
            manifest=json.loads((temp/'native/resource.json').read_text())
            converted=[]
            for tag, geometry in f.converter.chunks(f.converter.one(f.converter.one(f.converter.read(temp/'native'/manifest['player']['dff']),16),26)):
                if tag != 15: continue
                body=f.converter.one(geometry,1); _,nt,nv,_=f.converter.S.unpack_from('<4I',body)
                offset=16+nv*12+nt*8+24
                converted.extend(f.converter.S.unpack_from('<3f',body,offset+i*12) for i in range(nv))
            self.assertEqual(len(converted),len(expected))
            for original, placed in zip(expected,converted):
                for a,b in zip(original,placed): self.assertAlmostEqual(a,b,places=5)
            if AUDIT.exists():
                result=subprocess.run([str(AUDIT),str(temp/'native'/manifest['player']['dff']),'--skin'],capture_output=True,text=True)
                self.assertEqual(result.returncode,0,result.stderr)

    def test_skinned_import_requires_rig_mapping_and_one_player_or_sixteen_clothes(self):
        with tempfile.TemporaryDirectory() as temp:
            temp=Path(temp); root=resource(temp/'input'); source,mapping,_=owned_clothing(root/'stream')
            with self.assertRaisesRegex(ValueError,'needs --base-player'):
                f.import_resource(root,temp/'no-rig','player')
            mapping.write_text('{}')
            with self.assertRaisesRegex(ValueError,'Missing bone mapping'):
                f.import_resource(root,temp/'bad-map','clothing',**self.rig(mapping))
            self.assertFalse((temp/'bad-map').exists())
            for i in range(17): (root/f'stream/item{i}.ydd.xml').write_bytes(source.read_bytes())
            for kind in ('player','clothing'):
                with self.assertRaisesRegex(ValueError,'model budget'):
                    f.import_resource(root,temp/kind,kind,**self.rig(mapping))
                self.assertFalse((temp/kind).exists())

    def test_explicit_clothing_texture_pairing_uses_nonmatching_dictionary_name(self):
        with tempfile.TemporaryDirectory() as temp:
            temp=Path(temp); root=resource(temp/'input'); source,mapping,_=owned_clothing(root/'stream')
            tree=f.converter.parse_xml(source)
            parameter=f.converter.ET.SubElement(f.converter.ET.SubElement(tree.find('ShaderGroup/Shaders/Item'),'Parameters'),'Item',name='DiffuseSampler')
            f.converter.ET.SubElement(parameter,'Name').text='shirt_diffuse'
            f.converter.ET.ElementTree(tree).write(source)
            (root/'textures').mkdir()
            (root/'textures/different_diff_000_a_uni.ytd.xml').write_text('<TextureDictionary/>')
            header=bytearray(128); header[:4]=b'DDS '
            f.converter.S.pack_into('<II',header,12,1,1)
            f.converter.S.pack_into('<7I',header,80,0x41,0,32,0xff,0xff00,0xff0000,0xff000000)
            (root/'textures/shirt_diffuse.dds').write_bytes(header+bytes([12,34,56,255]))
            report=f.import_resource(root,temp/'clothes','clothing',texture_map=['stream/jacket.ydd.xml=textures/different_diff_000_a_uni.ytd.xml'],**self.rig(mapping))
            self.assertEqual(report['converted'][0]['textures'],1)
            self.assertEqual(report['texture_overrides']['stream/jacket.ydd.xml'],'textures/different_diff_000_a_uni.ytd.xml')

    def test_external_skeleton_is_copied_from_selected_resource_data_only(self):
        with tempfile.TemporaryDirectory() as temp:
            temp=Path(temp); root=resource(temp/'input'); source,mapping,_=owned_clothing(root/'stream')
            tree=f.converter.parse_xml(source); bones=tree.find('Skeleton'); tree.remove(bones)
            f.converter.ET.ElementTree(tree).write(source)
            fragment=f.converter.ET.Element('Fragment'); drawable=f.converter.ET.SubElement(fragment,'Drawable'); drawable.append(bones)
            f.converter.ET.ElementTree(fragment).write(root/'stream/rig.yft.xml')
            report=f.import_resource(root,temp/'clothes','clothing',skeleton='stream/rig.yft.xml',**self.rig(mapping))
            self.assertGreater(report['converted'][0]['vertices'],0)
            for skeleton in ('../outside.xml','client.lua'):
                with self.assertRaisesRegex(ValueError,'exact relative CodeWalker'):
                    f.import_resource(root,temp/'bad','clothing',skeleton=skeleton,**self.rig(mapping))

    def test_texture_pairing_rejects_missing_unselected_duplicate_and_escaping_paths(self):
        with tempfile.TemporaryDirectory() as temp:
            temp=Path(temp); root=resource(temp/'input'); source,mapping,_=owned_clothing(root/'stream')
            (root/'stream/texture.ytd.xml').write_text('<TextureDictionary/>')
            for pairs in (['missing.ydd=stream/texture.ytd.xml'],['stream/jacket.ydd.xml=../outside.ytd'],
                          ['stream/jacket.ydd.xml=stream/texture.ytd.xml']*2):
                with self.assertRaisesRegex(ValueError,'--texture'):
                    f.import_resource(root,temp/'bad','clothing',texture_map=pairs,**self.rig(mapping))
                self.assertFalse((temp/'bad').exists())

    def test_static_ytyp_alias_names_and_hashes_resolve_to_the_selected_drawable(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp); root = resource(temp / 'input')
            mesh(root / 'stream/prop.ydr.xml')
            types = root / 'stream/types.ytyp.xml'
            types.write_text('''<CMapTypes><archetypes><Item type="CBaseArchetypeDef">
                <name>fixture_alias</name><assetName>prop</assetName>
                <assetType>ASSET_TYPE_DRAWABLE</assetType><extensions/>
                </Item></archetypes><extensions/><compositeEntityTypes/></CMapTypes>''')
            for index, alias in enumerate(('fixture_alias', f'hash_{f.map_converter.jenkins("fixture_alias"):08X}')):
                map_xml(root / 'stream/map.ymap.xml', archetype=alias)
                output = temp / f'native-{index}'
                report = f.import_resource(root, output, 'map', ymap='stream/map.ymap.xml', ytyp=['stream/types.ytyp.xml'])
                self.assertEqual(report['map']['archetype_aliases'], 1)
                self.assertEqual(json.loads((output / 'resource.json').read_text())['placements'][0]['model_id'], 30000)

    @unittest.skipUnless((ROOT / 'tools/tests/map-fixture/bin/Release/net9.0/MapFixture.dll').exists(), 'Build MapFixture')
    def test_owned_binary_ytyp_alias_preserves_asset_mapping(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp); root = resource(temp / 'input')
            mesh(root / 'stream/prop.ydr.xml')
            map_xml(root / 'stream/map.ymap.xml', archetype='fixture_alias')
            types = root / 'stream/types.ytyp.xml'
            types.write_text('<CMapTypes><archetypes><Item type="CBaseArchetypeDef"><name>fixture_alias</name><assetName>prop</assetName><assetType>ASSET_TYPE_DRAWABLE</assetType></Item></archetypes></CMapTypes>')
            helper = ROOT / 'tools/tests/map-fixture/bin/Release/net9.0/MapFixture.dll'
            binary = root / 'stream/types.ytyp'
            result = subprocess.run(['dotnet', str(helper), str(types), str(binary)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(binary.read_bytes()[:4], b'RSC7')
            report = f.import_resource(root, temp / 'binary', 'map', ymap='stream/map.ymap.xml', ytyp=['stream/types.ytyp'])
            self.assertEqual(report['map']['archetype_aliases'], 1)

    def test_ytyp_texture_dictionary_with_a_different_basename_is_used(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp); root = resource(temp / 'input')
            model = mesh(root / 'stream/prop.ydr.xml')
            tree = f.converter.parse_xml(model)
            parameters = f.converter.ET.SubElement(tree.find('ShaderGroup/Shaders/Item'), 'Parameters')
            parameter = f.converter.ET.SubElement(parameters, 'Item', name='DiffuseSampler')
            f.converter.ET.SubElement(parameter, 'Name').text = 'fixture'
            f.converter.ET.ElementTree(tree).write(model)
            (root / 'textures').mkdir()
            (root / 'textures/shared.ytd.xml').write_text('<TextureDictionary/>')
            header = bytearray(128); header[:4] = b'DDS '
            f.converter.S.pack_into('<II', header, 12, 1, 1)
            f.converter.S.pack_into('<7I', header, 80, 0x41, 0, 32, 0xff, 0xff00, 0xff0000, 0xff000000)
            (root / 'textures/fixture.dds').write_bytes(header + bytes([12, 34, 56, 255]))
            types = root / 'stream/types.ytyp.xml'
            types.write_text('<CMapTypes><archetypes><Item type="CBaseArchetypeDef"><name>alias</name><assetName>prop</assetName><assetType>ASSET_TYPE_DRAWABLE</assetType><textureDictionary>shared</textureDictionary></Item></archetypes></CMapTypes>')
            map_xml(root / 'stream/map.ymap.xml', archetype='alias')
            report = f.import_resource(root, temp / 'native', 'map', ymap='stream/map.ymap.xml', ytyp=['stream/types.ytyp.xml'])
            self.assertEqual(report['converted'][0]['textures'], 1)
            manifest = json.loads((temp / 'native/resource.json').read_text())
            dictionary = (temp / 'native' / manifest['models'][0]['txd']).read_bytes()
            self.assertIn(bytes([56, 34, 12, 255]), dictionary)

    def test_ytyp_unsupported_semantics_duplicates_missing_assets_and_dictionary_fail_atomically(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp); root = resource(temp / 'input')
            mesh(root / 'stream/prop.ydr.xml')
            mesh(root / 'stream/other.ydr.xml')
            map_xml(root / 'stream/map.ymap.xml', archetype='alias')
            types = root / 'stream/types.ytyp.xml'
            base = '<Item type="CBaseArchetypeDef"><name>alias</name><assetName>prop</assetName><assetType>ASSET_TYPE_DRAWABLE</assetType></Item>'
            for index, (entry, error) in enumerate([
                (base.replace('CBaseArchetypeDef', 'CMloArchetypeDef'), 'MLO/time'),
                (base.replace('CBaseArchetypeDef', 'CTimeArchetypeDef'), 'MLO/time'),
                (base.replace('ASSET_TYPE_DRAWABLE', 'ASSET_TYPE_DRAWABLEDICTIONARY'), 'individual DRAWABLE'),
                (base.replace('<assetName>prop', '<assetName>missing'), 'Missing selected YDR'),
                (base + base, 'Duplicate YTYP'),
                (base.replace('<name>alias', '<name>prop').replace('<assetName>prop', '<assetName>other'), 'conflicts'),
                (base.replace('</Item>', '<extensions><Item type="CExtensionDefDoor"/></extensions></Item>'), 'extensions'),
                (base.replace('</Item>', '<textureDictionary>missing_texture</textureDictionary></Item>'), 'Missing YTD'),
            ]):
                types.write_text('<CMapTypes><archetypes>' + entry + '</archetypes></CMapTypes>')
                output = temp / f'bad-{index}'
                with self.assertRaisesRegex(ValueError, error):
                    f.import_resource(root, output, 'map', ymap='stream/map.ymap.xml', ytyp=['stream/types.ytyp.xml'])
                self.assertFalse(output.exists())
            with self.assertRaisesRegex(ValueError, 'exact relative'):
                f.import_resource(root, temp / 'outside', 'map', ymap='stream/map.ymap.xml', ytyp=['../escape.ytyp.xml'])

    def test_emitted_map_uses_the_native_placement_budget(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp); root = resource(temp / 'input')
            mesh(root / 'stream/prop.ydr.xml')
            path = map_xml(root / 'stream/map.ymap.xml')
            tree = f.converter.parse_xml(path)
            entities = tree.find('entities')
            template = f.converter.ET.tostring(entities[0])
            for _ in range(2000):
                entities.append(f.converter.ET.fromstring(template))
            f.converter.ET.ElementTree(tree).write(path)
            with self.assertRaisesRegex(ValueError, 'native 2000 placement'):
                f.import_resource(root, temp / 'native', 'map', ymap='stream/map.ymap.xml')
            self.assertFalse((temp / 'native').exists())
    @unittest.skipUnless((ROOT / 'tools/tests/map-fixture/bin/Release/net9.0/MapFixture.dll').exists(),
                         'Build MapFixture to exercise owned binary YMAP round-trip')
    def test_owned_binary_ymap_preserves_placements_through_legacy_decoder(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp); root = resource(temp / 'input')
            mesh(root / 'stream/prop.ydr.xml')
            source = map_xml(root / 'source.ymap.xml', rotation='0 0 0.70710678 0.70710678')
            binary = root / 'stream/map.ymap'
            helper = ROOT / 'tools/tests/map-fixture/bin/Release/net9.0/MapFixture.dll'
            result = subprocess.run(['dotnet', str(helper), str(source), str(binary)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(binary.read_bytes()[:4], b'RSC7')
            f.import_resource(root, temp / 'binary', 'map', ymap='stream/map.ymap', offset=[2., 3., 4.])
            f.import_resource(root, temp / 'xml', 'map', ymap='source.ymap.xml', offset=[2., 3., 4.])
            a = json.loads((temp / 'binary/resource.json').read_text())
            b = json.loads((temp / 'xml/resource.json').read_text())
            self.assertEqual(a['placements'][0]['model_id'], b['placements'][0]['model_id'])
            self.assertEqual(a['placements'][0]['position'], [12., 23., 7.])
            for raw, xml in zip(a['placements'][0]['rotation'], b['placements'][0]['rotation']):
                self.assertAlmostEqual(raw, xml, places=6)

    @unittest.skipUnless((ROOT / 'tools/gta5-extract/bin/Release/net9.0/Gta5Extract.dll').exists(), 'Build extractor')
    def test_raw_ymap_bad_header_and_excessive_declared_allocation_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp); root = resource(temp / 'input')
            mesh(root / 'stream/prop.ydr.xml')
            path = root / 'stream/map.ymap'
            for index, data in enumerate([b'RSC8' + bytes(12), b'RSC7' + bytes(4) + bytes([255])*8]):
                path.write_bytes(data)
                output = temp / f'bad-{index}'
                with self.assertRaises(f.converter.subprocess.CalledProcessError):
                    f.import_resource(root, output, 'map', ymap='stream/map.ymap')
                self.assertFalse(output.exists())

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
            self.assertEqual([v['name'] for v in manifest['vehicles']], ['first', 'second'])
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
