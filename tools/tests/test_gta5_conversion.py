"""Owned/generated fixtures only; no downloaded GTA V models in the test suite."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import struct as S
import subprocess
import tempfile
from types import SimpleNamespace
import unittest
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('convert_gta5', ROOT / 'tools/convert-gta5.py')
c = importlib.util.module_from_spec(spec)
spec.loader.exec_module(c)


def args(source, output, **options):
    defaults = dict(input=source, out=output, type='map', textures=None, skeleton=None,
                    base_player=None, base_ifp=None, base_txd=None, bone_map=None,
                    scale=1., flip_v=False, enable=False, model_id=30000, position=[0.,0.,0.])
    defaults.update(options)
    return SimpleNamespace(**defaults)


def owned_clothing(directory, original=None):
    """Express an owned skinned mesh in a shifted GTA V XML rig to exercise retargeting."""
    original = original or ROOT / 'mods/native-clothing-demo/jacket.dff'
    clump = c.one(c.read(original),16)
    _, ids, binds = c.native_rig(original)
    drawable = ET.Element('Drawable')
    bones = ET.SubElement(ET.SubElement(drawable,'Skeleton'),'Bones')
    mapping = {}
    for index, (id_, bind) in enumerate(zip(ids,binds)):
        bone = ET.SubElement(bones,'Item')
        ET.SubElement(bone,'Name').text = f'fixture_{id_}'
        for name, number in [('Index',index),('Tag',1000+id_),('ParentIndex',-1)]:
            ET.SubElement(bone,name,value=str(number))
        world = c.inverse(bind)
        ET.SubElement(bone,'Translation',x=str(world[12]+2),y=str(world[13]),z=str(world[14]))
        ET.SubElement(bone,'Rotation',x='0',y='0',z='0',w='1')
        ET.SubElement(bone,'Scale',x='1',y='1',z='1')
        mapping[f'fixture_{id_}'] = id_
    shaders = ET.SubElement(ET.SubElement(drawable,'ShaderGroup'),'Shaders')
    ET.SubElement(shaders,'Item')
    model = ET.SubElement(ET.SubElement(drawable,'DrawableModelsHigh'),'Item')
    ET.SubElement(model,'HasSkin',value='1')
    geometries = ET.SubElement(model,'Geometries')
    expected = []
    for tag, geometry in c.chunks(c.one(clump,26)):
        if tag != 15:
            continue
        body = c.one(geometry,1)
        flags, nt, nv, morph = S.unpack_from('<4I',body)
        offset = 16 + nv*4
        triangles = [S.unpack_from('<4H',body,offset+i*8) for i in range(nt)]
        offset += nt*8 + 24
        positions = [S.unpack_from('<3f',body,offset+i*12) for i in range(nv)]
        expected.extend(positions)
        skin = c.one(c.one(geometry,3),0x116)
        skin_offset = 4+skin[1]
        mesh = ET.SubElement(geometries,'Item')
        ET.SubElement(mesh,'BoneIDs').text = ', '.join(map(str,range(len(ids))))
        ET.SubElement(mesh,'ShaderIndex',value='0')
        buffer = ET.SubElement(mesh,'VertexBuffer'); layout = ET.SubElement(buffer,'Layout',type='GTAV1')
        for field in ['Position','BlendWeights','BlendIndices','Normal','Colour0','TexCoord0']:
            ET.SubElement(layout,field)
        rows = []
        for i, position in enumerate(positions):
            indices = list(skin[skin_offset+i*4:skin_offset+i*4+4])
            weights = [round(weight * 255) for weight in S.unpack_from('<4f',skin,skin_offset+nv*4+i*16)]
            colour = list(body[16+i*4:16+i*4+4])
            row = [position[0]+2,*position[1:],*weights,*indices,0.,1.,0.,*colour,0.,0.]
            rows.append(' '.join(map(str,row)))
        ET.SubElement(buffer,'Data').text = '\n'.join(rows)
        ET.SubElement(ET.SubElement(mesh,'IndexBuffer'),'Data').text = ' '.join(str(v) for b,a,mat,z in triangles for v in (a,b,z))
    source = directory/'jacket.ydd.xml'; ET.ElementTree(drawable).write(source)
    bone_map = directory/'bones.json';bone_map.write_text(json.dumps(mapping))
    return source, bone_map, expected


AUDIT = ROOT / 'native/target/debug/examples' / ('audit_resource.exe' if os.name == 'nt' else 'audit_resource')

class ConversionTests(unittest.TestCase):
    def test_glass_file_identifiers_preserve_transparency_in_native_decoder(self):
        with tempfile.TemporaryDirectory() as temp:
            temp=Path(temp)
            for index, (name, bucket, expected) in enumerate([
                    ('vehicle_vehglass.sps',1,128),('hash_B88DC892',1,128),
                    ('vehicle_vehglass_inner.sps',0,128),('glass.sps',0,128),
                    ('vehicle_lights.sps',1,255),('vehicle_paint3.sps',0,255),
                    ('unknown.sps',2,128),('hash_NOTHEX',0,255)]):
                source=temp/f'mesh-{index}.ydr.xml'
                source.write_text(f"""<Drawable><ShaderGroup><Shaders><Item>
                  <FileName>{name}</FileName><RenderBucket value="{bucket}"/>
                  </Item></Shaders></ShaderGroup><DrawableModelsHigh><Item><Geometries><Item>
                  <ShaderIndex value="0"/><VertexBuffer><Layout><Position/><Normal/></Layout>
                  <Data>0 0 0 0 0 1\n1 0 0 0 0 1\n0 1 0 0 0 1</Data></VertexBuffer>
                  <IndexBuffer><Data>0 1 2</Data></IndexBuffer>
                  </Item></Geometries></Item></DrawableModelsHigh></Drawable>""")
                output=temp/f'native-{index}'
                with contextlib.redirect_stdout(io.StringIO()):
                    c.convert(args(source,output))
                geometry=c.one(c.one(c.one(c.read(output/'stream/converted.dff'),16),26),15)
                material=c.one(c.one(geometry,8),7)
                self.assertEqual(S.unpack_from('<I4B',c.one(material,1))[4],expected)
                if AUDIT.exists():
                    result=subprocess.run([str(AUDIT),str(output/'stream/converted.dff'),'--materials'],capture_output=True,text=True)
                    self.assertEqual(result.returncode,0,result.stderr)
                    colors=[json.loads(line)['color'] for line in result.stdout.splitlines() if line.startswith('{')]
                    self.assertEqual(colors,[[255,255,255,expected]])
            for name, number in c.GLASS_SHADERS.items():
                for text in (name, f'hash_{number:08X}'):
                    self.assertTrue(c.is_glass(ET.fromstring(f'<Item><FileName>{text}</FileName></Item>')))

    def test_inverse_and_normal_under_nonuniform_scale(self):
        matrix = c.IDENTITY.copy(); matrix[0]=2.; matrix[5]=3.; matrix[12]=7.
        self.assertEqual(c.transform(c.inverse(matrix),c.transform(matrix,[1.,2.,3.])),[1.,2.,3.])
        self.assertEqual(c.normal_transform(matrix,[1.,1.,0.]),[.5,1/3,0.])
        with self.assertRaisesRegex(ValueError,'Singular'):
            c.inverse([0.]*16)

    def test_dds_rgba_and_truncation(self):
        header = bytearray(128);header[:4]=b'DDS '
        S.pack_into('<II',header,12,1,1)
        S.pack_into('<7I',header,80,0x41,0,32,0xff,0xff00,0xff0000,0xff000000)
        texture = c.native_texture('fixture',header+bytes([1,2,3,4]))
        self.assertEqual(c.one(c.one(texture,21),1)[92:],bytes([3,2,1,4]))
        with self.assertRaisesRegex(ValueError,'Truncated'):
            c.native_texture('fixture',header+bytes([1,2]))
        S.pack_into('<I',header,84,int.from_bytes(b'DX10','little'));S.pack_into('<I',header,80,4)
        with self.assertRaisesRegex(ValueError,'BC7/DX10'):
            c.native_texture('fixture',header+bytes(8))

    def test_xml_entities_and_cycles(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'bad.xml';path.write_text('<!DOCTYPE x [<!ENTITY a "b">]><Drawable/>')
            with self.assertRaisesRegex(ValueError,'DTD/entity'):c.parse_xml(path)
        root=ET.fromstring('<Drawable><Skeleton><Bones><Item><Index value="0"/><ParentIndex value="0"/></Item></Bones></Skeleton></Drawable>')
        with self.assertRaisesRegex(ValueError,'cyclic'):c.skeleton(root)

    def test_invalid_triangle_index(self):
        vertex=dict(pos=[0.,0.,0.],normal=[0.,0.,1.],uv=[0.,0.],colour=[255]*4)
        with self.assertRaisesRegex(ValueError,'triangle indices'):
            c.geometry([vertex],[0,1,0],None)

    def test_shared_wheels_and_pristine_offsets(self):
        root=ET.fromstring('<Fragment><Drawable/><Physics><LOD1><PositionOffset x="0" y="0" z="1"/><Children/><Transforms/></LOD1></Physics></Fragment>')
        children=root.find('Physics/LOD1/Children');matrices=root.find('Physics/LOD1/Transforms')
        for i,tag in enumerate([26418,26398,27902,27922]):
            child=ET.SubElement(children,'Item');ET.SubElement(child,'BoneTag',value=str(tag))
            drawable=ET.SubElement(child,'Drawable')
            if tag==27922:
                ET.SubElement(ET.SubElement(ET.SubElement(ET.SubElement(drawable,'DrawableModelsHigh'),'Item'),'Geometries'),'Item')
            m=c.IDENTITY.copy();m[12]=i;ET.SubElement(matrices,'Item').text=' '.join(map(str,m))
        jobs=c.fragment_children(root,{'warnings':[]})
        self.assertEqual(len(jobs),4)
        self.assertEqual(c.transform(jobs[0][1],[1.,0.,0.]),[-1.,0.,1.])
        self.assertEqual(c.transform(jobs[3][1],[1.,0.,0.]),[4.,0.,1.])

    def test_clothing_retargets_to_native_bind_and_refuses_overwrite(self):
        with tempfile.TemporaryDirectory() as directory:
            directory=Path(directory);source,mapping,expected=owned_clothing(directory)
            options=args(source,directory/'converted',type='clothing',base_player=ROOT/'mods/native-ped-demo/ped.dff',base_ifp=ROOT/'mods/native-ped-demo/ped.ifp',bone_map=mapping)
            with contextlib.redirect_stdout(io.StringIO()):c.convert(options)
            frames,ids,binds=c.native_rig(options.out/'stream/converted.dff')
            self.assertEqual(ids,c.native_rig(options.base_player)[1]);self.assertEqual(binds,c.native_rig(options.base_player)[2])
            result=[]
            for tag, geometry in c.chunks(c.one(c.one(c.read(options.out/'stream/converted.dff'),16),26)):
                if tag != 15:continue
                body=c.one(geometry,1);_,nt,nv,_=S.unpack_from('<4I',body)
                offset=16+nv*12+nt*8+24
                result.extend(S.unpack_from('<3f',body,offset+i*12)for i in range(nv))
            self.assertEqual(len(expected),len(result))
            for source_vertex,converted in zip(expected,result):
                for a,b in zip(source_vertex,converted):self.assertAlmostEqual(a,b,places=5)
            with self.assertRaisesRegex(ValueError,'Output already exists'):c.convert(options)

    @unittest.skipUnless(AUDIT.exists(), 'build sa-assets example audit_resource to check native decoding')
    def test_native_decoder_accepts_rigid_and_skinned_conversion(self):
        with tempfile.TemporaryDirectory() as directory:
            directory=Path(directory);source,mapping,_=owned_clothing(directory)
            for kind in ['map','clothing']:
                options=args(source,directory/kind,type=kind,base_player=ROOT/'mods/native-ped-demo/ped.dff',base_ifp=ROOT/'mods/native-ped-demo/ped.ifp',bone_map=mapping)
                with contextlib.redirect_stdout(io.StringIO()):c.convert(options)
                command=[str(AUDIT),str(options.out/'stream/converted.dff')]
                if kind=='clothing':command.append('--skin')
                result=subprocess.run(command,capture_output=True,text=True)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertIn('Native decoder accepted',result.stdout)

    def test_full_player_conversion_registers_converted_mesh_and_native_animation(self):
        with tempfile.TemporaryDirectory() as directory:
            directory=Path(directory)
            source,mapping,expected=owned_clothing(directory,ROOT/'mods/native-ped-demo/ped.dff')
            options=args(source,directory/'player',type='player',base_player=ROOT/'mods/native-ped-demo/ped.dff',base_ifp=ROOT/'mods/native-ped-demo/ped.ifp',bone_map=mapping,enable=True)
            with contextlib.redirect_stdout(io.StringIO()):c.convert(options)
            manifest=json.loads((options.out/'resource.json').read_text())
            self.assertEqual(manifest['player'],{'dff':'stream/converted.dff','ifp':'stream/base.ifp'})
            self.assertEqual((options.out/'stream/base.ifp').read_bytes(),options.base_ifp.read_bytes())
            self.assertFalse((options.out/'stream/base.dff').exists())
            self.assertNotIn('clothes',manifest['player'])
            self.assertEqual(c.native_rig(options.out/'stream/converted.dff')[1:],c.native_rig(options.base_player)[1:])
            if AUDIT.exists():
                result=subprocess.run([str(AUDIT),str(options.out/'stream/converted.dff'),'--skin'],capture_output=True,text=True)
                self.assertEqual(result.returncode,0,result.stderr)

    def test_missing_weighted_bone_mapping_does_not_publish_resource(self):
        with tempfile.TemporaryDirectory() as directory:
            directory=Path(directory);source,mapping,_=owned_clothing(directory);mapping.write_text('{}')
            options=args(source,directory/'converted',type='clothing',base_player=ROOT/'mods/native-ped-demo/ped.dff',base_ifp=ROOT/'mods/native-ped-demo/ped.ifp',bone_map=mapping)
            with self.assertRaisesRegex(ValueError,'Missing bone mapping'):c.convert(options)
            self.assertFalse(options.out.exists())

if __name__=='__main__':unittest.main()
