#!/usr/bin/env python3
"""Regenerate the SARE application icon from the bundled OFL title font.

Outputs (committed, so builds need neither Python nor Pillow):
  native/assets/icon.png        256 px master, also used in the README
  native/assets/icon-128.rgba   raw RGBA for the launcher/runtime window icon
  native/assets/sare.ico        Windows shortcut icon
  native/assets/sare.res        compiled Windows resource linked into the .exe files
Requires Pillow: python tools/make-icon.py
"""
import io
from pathlib import Path
import struct

from PIL import Image, ImageDraw, ImageFont

REPO = Path(__file__).resolve().parents[1]
FONT = REPO / 'native/crates/runtime/assets/UnifrakturCook-Bold.ttf'
OUT = REPO / 'native/assets'
GOLD, DARK, EDGE = (223, 183, 120, 255), (17, 15, 12, 255), (120, 93, 47, 255)
SIZES = (16, 24, 32, 48, 64, 256)


def master(size=256, scale=4):
    side = size * scale
    image = Image.new('RGBA', (side, side), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    pad, radius = side * 0.04, side * 0.2
    draw.rounded_rectangle((pad, pad, side - pad, side - pad), radius, fill=DARK, outline=EDGE, width=int(side * 0.035))
    font = ImageFont.truetype(str(FONT), int(side * 0.5))
    left, top, right, bottom = draw.textbbox((0, 0), 'SA', font=font)
    draw.text(((side - (right - left)) / 2 - left, (side - (bottom - top)) / 2 - top), 'SA', font=font, fill=GOLD)
    return image.resize((size, size), Image.LANCZOS)


def dib(image):
    """32-bit icon bitmap: bottom-up BGRA followed by an all-visible AND mask."""
    size = image.width
    rows = [image.crop((0, y, size, y + 1)).tobytes('raw', 'BGRA') for y in reversed(range(size))]
    mask_row = bytes(((size + 31) // 32) * 4)
    header = struct.pack('<IiiHHIIiiII', 40, size, size * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    return header + b''.join(rows) + mask_row * size


def png(image):
    data = io.BytesIO()
    image.save(data, 'PNG', optimize=True)
    return data.getvalue()


def resource(kind, name, data, flags):
    header = struct.pack('<IIHHHHIHHII', len(data), 32, 0xffff, kind, 0xffff, name, 0, flags, 0x0409, 0, 0)
    return header + data + bytes(-len(data) % 4)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    source = master()
    source.save(OUT / 'icon.png', optimize=True)
    (OUT / 'icon-128.rgba').write_bytes(source.resize((128, 128), Image.LANCZOS).tobytes())
    images = [(size, source if size == 256 else source.resize((size, size), Image.LANCZOS)) for size in SIZES]
    payloads = [(size, png(image) if size == 256 else dib(image)) for size, image in images]
    # .ico: directory entries carry file offsets.
    offset = 6 + 16 * len(payloads)
    ico = struct.pack('<HHH', 0, 1, len(payloads))
    for size, data in payloads:
        ico += struct.pack('<BBBBHHII', size % 256, size % 256, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    (OUT / 'sare.ico').write_bytes(ico + b''.join(data for _, data in payloads))
    # .res: an empty marker entry, one RT_ICON (3) per image, then RT_GROUP_ICON (14)
    # whose entries carry resource ids. Name 1 is the icon Explorer shows.
    res = struct.pack('<IIHHHHIHHII', 0, 32, 0xffff, 0, 0xffff, 0, 0, 0, 0, 0, 0)
    group = struct.pack('<HHH', 0, 1, len(payloads))
    for index, (size, data) in enumerate(payloads, start=1):
        res += resource(3, index, data, 0x1010)
        group += struct.pack('<BBBBHHIH', size % 256, size % 256, 0, 0, 1, 32, len(data), index)
    res += resource(14, 1, group, 0x1030)
    (OUT / 'sare.res').write_bytes(res)
    print('Wrote', ', '.join(sorted(p.name for p in OUT.iterdir())))


if __name__ == '__main__':
    main()
