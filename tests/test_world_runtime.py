import json
import math
from pathlib import Path
import struct
import sys
import tempfile
import unittest

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
from build_scene import place
from load_assets import png
from native_textures import decode_blocks, decode_bgra, decode_native_txd
from static_geometry import decode_static_dff
from mods import load_mods


def chunk(tag,body):
    return struct.pack('<III',tag,len(body),0x1803FFFF)+body


def synthetic_dff(material_index=0,frame_parent=-1,light=False):
    material=chunk(7,chunk(1,struct.pack('<I4BII3f',0,255,255,255,255,0,0,1,1,1)))
    ml=chunk(8,chunk(1,struct.pack('<Iii',2,-1,0))+material)
    # Two material slots share one material; one triangle uses a selected slot.
    body=struct.pack('<4I',0,1,3,1)+struct.pack('<4H',1,0,material_index,2)
    body+=struct.pack('<4fII',0,0,0,1,1,0)
    body+=struct.pack('<9f',0,0,0,1,0,0,0,1,0)
    geom=chunk(15,chunk(1,body)+ml)
    transform=struct.pack('<I12fiI',1,1,0,0,0,1,0,0,0,1,2,3,4,frame_parent,0)
    frame=chunk(14,chunk(1,transform)+chunk(3,b''))
    geo=chunk(26,chunk(1,struct.pack('<I',1))+geom)
    atomic=chunk(20,chunk(1,struct.pack('<4I',0,0,4,0))+chunk(3,b''))
    extra=chunk(1,struct.pack('<I',0))+chunk(18,b'') if light else b''
    return chunk(16,chunk(1,struct.pack('<3I',1,int(light),0))+frame+geo+atomic+extra+chunk(3,b''))


class StaticGeometryTests(unittest.TestCase):
    def test_material_reuse_and_frame_translation(self):
        g=decode_static_dff(synthetic_dff(material_index=1))[0]
        self.assertEqual(g['vertices'][1],[3,3,4])
        self.assertEqual(g['triangles'],[[0,1,2,1]])
        self.assertEqual(g['materials'][0],g['materials'][1])

    def test_attached_light_struct_is_not_clump_header(self):
        self.assertEqual(len(decode_static_dff(synthetic_dff(light=True))),1)

    def test_invalid_material_and_frame_reference(self):
        for data in (synthetic_dff(material_index=2),synthetic_dff(frame_parent=0)):
            with self.assertRaises(ValueError):decode_static_dff(data)

    def test_inverse_ipl_rotation_translation_and_axis_conversion(self):
        row={'position':[10,20,30],'rotation':[0,0,math.sqrt(.5),math.sqrt(.5)]}
        v=place([1,0,0],row,[10,20])
        for a,b in zip(v,[0,30,1]):self.assertAlmostEqual(a,b,places=6)


class NativeTextureTests(unittest.TestCase):
    def test_dxt1_opaque_and_transparent_selector(self):
        data=struct.pack('<HHI',0,65535,0xffffffff)
        self.assertEqual(decode_blocks(data,1,1,'DXT1',True),bytes([0,0,0,0]))
        self.assertEqual(decode_blocks(data,1,1,'DXT1',False),bytes([0,0,0,255]))

    def test_dxt3_explicit_alpha_and_four_color_mode(self):
        data=struct.pack('<QHHI',0xaaaaaaaaaaaaaaaa,0,65535,0xffffffff)
        self.assertEqual(decode_blocks(data,1,1,'DXT3'),bytes([170,170,170,170]))

    def test_dxt5_interpolated_alpha_and_special_alpha(self):
        color=struct.pack('<HHI',0xf800,0,0)
        interpolated=bytes([255,0])+(2).to_bytes(6,'little')+color
        special=bytes([0,255])+(7).to_bytes(6,'little')+color
        self.assertEqual(decode_blocks(interpolated,1,1,'DXT5'),bytes([255,0,0,218]))
        self.assertEqual(decode_blocks(special,1,1,'DXT5'),bytes([255,0,0,255]))

    def test_bgra_channel_order_and_unused_alpha(self):
        self.assertEqual(decode_bgra(bytes([10,20,30,40]),1,1,True),bytes([30,20,10,40]))
        self.assertEqual(decode_bgra(bytes([10,20,30,40]),1,1,False),bytes([30,20,10,255]))

    def test_truncated_blocks_rejected(self):
        for fmt in ('DXT1','DXT3','DXT5'):
            with self.assertRaises(ValueError):decode_blocks(b'',4,4,fmt)

    def test_dictionary_mip_length_is_checked(self):
        header=struct.pack('<II32s32sIIHHBBBB',9,0,b'red',b'',0x200,827611204,4,4,16,1,4,8)
        native=chunk(21,chunk(1,header+struct.pack('<I',8)+struct.pack('<HHI',0xf800,0,0)))
        data=chunk(22,chunk(1,struct.pack('<HH',1,2))+native)
        self.assertEqual(decode_native_txd(data)['red']['rgba'][:4],bytes([255,0,0,255]))
        bad=chunk(21,chunk(1,header+struct.pack('<I',7)+b'\0'*7))
        with self.assertRaises(ValueError):decode_native_txd(chunk(22,chunk(1,struct.pack('<HH',1,2))+bad))


class ModTests(unittest.TestCase):
    def manifest(self,root,folder_name,**fields):
        folder=root/folder_name;folder.mkdir()
        (folder/'mod.json').write_text(json.dumps({'schema_version':1,**fields}))
        return folder

    def test_order_and_disabled_mod(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d)
            self.manifest(root,'a',name='One',settings={'camera_speed':10})
            self.manifest(root,'b',name='Two',settings={'camera_speed':20})
            self.manifest(root,'c',enabled=False,settings={'camera_speed':99})
            m=load_mods(root)
            self.assertEqual(m['settings']['camera_speed'],20)
            self.assertEqual(m['active'],['One','Two'])
            self.assertFalse(load_mods(root,False)['active'])

    def test_own_texture_png_override(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d)
            folder=self.manifest(root,'a',texture_overrides={'test:red':'red.png'})
            data=png({'width':1,'height':1,'rgba':b'\xff\x00\x00\xff'})
            (folder/'red.png').write_bytes(data)
            m=load_mods(root)
            self.assertEqual(m['textures']['test:red'],(data,1,1))

    def test_path_escape_rejected(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d)/'mods';root.mkdir()
            (Path(d)/'outside.png').write_bytes(png({'width':1,'height':1,'rgba':b'\xff'*4}))
            self.manifest(root,'a',texture_overrides={'test:red':'../../outside.png'})
            with self.assertRaises(ValueError):load_mods(root)

    def test_nonfinite_settings_and_executable_fields_rejected(self):
        for fields in ({'settings':{'camera_speed':float('nan')}},{'script':'code.py'},{'placements':[{'model_id':1,'position':[0,0,0],'rotation':[0,0,0,0]}]}):
            with tempfile.TemporaryDirectory() as d:
                root=Path(d);self.manifest(root,'a',**fields)
                with self.assertRaises(ValueError):load_mods(root)


if __name__=='__main__':unittest.main()
