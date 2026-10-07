import struct
import sys
from pathlib import Path
import unittest

sys.path.insert(0, str(Path(__file__).parents[1] / 'tools'))
from load_assets import Reader, chunks, dxt1, root, png


class LoaderTests(unittest.TestCase):
    def test_truncated_chunks_rejected(self):
        for data in (b'abc', struct.pack('<III', 1, 10, 0x1803ffff) + b'abc'):
            with self.assertRaises(ValueError):
                list(chunks(data))

    def test_root_ignores_sector_padding_but_checks_body(self):
        data = struct.pack('<III', 0x10, 4, 0x1803ffff) + b'abcd' + b'\0'*20
        self.assertEqual(bytes(root(data, 0x10)), b'abcd')
        with self.assertRaises(ValueError):
            root(data, 0x16)

    def test_dxt1_red_block(self):
        data = struct.pack('<HHI', 0xf800, 0x07e0, 0)
        self.assertEqual(dxt1(data, 4, 4), bytes([255,0,0,255])*16)

    def test_dxt1_transparent_block(self):
        data = struct.pack('<HHI', 0, 0xffff, 0xffffffff)
        self.assertEqual(dxt1(data, 4, 4), bytes(64))

    def test_dxt1_partial_block(self):
        data = struct.pack('<HHI', 0xf800, 0x07e0, 0)
        self.assertEqual(len(dxt1(data, 1, 2)), 8)

    def test_invalid_dxt1_length_or_size_rejected(self):
        for data, w, h in ((b'',4,4),(bytes(8),0,4),(bytes(8),8192,4)):
            with self.assertRaises(ValueError):
                dxt1(data,w,h)

    def test_png_signature(self):
        data=png({'width':1,'height':1,'rgba':bytes([255,0,0,255])})
        self.assertTrue(data.startswith(b'\x89PNG\r\n\x1a\n'))


if __name__ == '__main__':
    unittest.main()
