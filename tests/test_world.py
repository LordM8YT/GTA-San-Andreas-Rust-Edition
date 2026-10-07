import math
from pathlib import Path
import struct
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'tools'))
from load_world import binary_instances, text_instances, registered_files


class WorldTests(unittest.TestCase):
    def binary(self, position=(1,2,3), offset=76, count=1):
        header = bytearray(76)
        struct.pack_into('<4sI', header, 0, b'bnry', count)
        struct.pack_into('<I', header, 28, offset)
        return bytes(header) + struct.pack('<7f3i', *position, 0,0,0,1, 1347,0,-1)

    def test_text_binary_equivalence(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d)/'test.ipl'
            p.write_text('# test\ninst\n1347, bin, 0, 1, 2, 3, 0, 0, 0, 1, -1\nend\n')
            a = text_instances(p,'test')[0]
        b = binary_instances(self.binary(),'test')[0]
        for key in ('id','interior','position','rotation','lod','source','index'):
            self.assertEqual(a[key],b[key])

    def test_invalid_binary_bounds(self):
        for data in (b'bnry',self.binary(offset=200),self.binary(count=2),self.binary(offset=0)):
            with self.assertRaises(ValueError):
                binary_instances(data,'test')

    def test_nonfinite_positions(self):
        with self.assertRaises(ValueError):
            binary_instances(self.binary(position=(math.nan,2,3)),'test')

    def test_invalid_quaternion(self):
        data=bytearray(self.binary())
        struct.pack_into('<f',data,76+24,0)
        with self.assertRaises(ValueError):
            binary_instances(data,'test')

    def test_registered_files_ignore_unlisted_conflicts_and_keep_manifest_order(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            maps = root/'data/maps'
            maps.mkdir(parents=True)
            for name in ('first.ide', 'second.ide', 'unused.ide'):
                (maps/name).write_text('objs\n16700, example, example, 1, 100, 0\nend\n')
            (root/'data/default.dat').write_text('IDE data/maps/first.ide\n')
            (root/'data/gta.dat').write_text('IDE data/maps/second.ide\n')
            self.assertEqual([p.name for p in registered_files(root, '.ide')],
                             ['first.ide', 'second.ide'])


if __name__ == '__main__':
    unittest.main()
