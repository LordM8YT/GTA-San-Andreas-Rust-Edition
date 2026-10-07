"""Classic PC D3D9 textures required by the first world region: BC1/2/3 and BGRA."""
import struct

from load_assets import Reader, root, chunks, one, cstring, rgb565


def decode_blocks(data,width,height,kind,has_alpha=True):
    if kind not in ('DXT1','DXT3','DXT5') or not 0 < width <= 4096 or not 0 < height <= 4096:
        raise ValueError('Unsupported block format/dimensions')
    blocksize = 8 if kind == 'DXT1' else 16
    if len(data) != ((width+3)//4)*((height+3)//4)*blocksize:
        raise ValueError('Invalid compressed texture size')
    out = bytearray(width*height*4)
    rd = Reader(data)
    for by in range(0,height,4):
        for bx in range(0,width,4):
            alpha = [255]*16
            if kind == 'DXT3':
                bits = rd.unpack('<Q')[0]
                alpha = [((bits>>(i*4))&15)*17 for i in range(16)]
            elif kind == 'DXT5':
                a,b = rd.unpack('<BB')
                selectors = int.from_bytes(rd.take(6),'little')
                values = [a,b]
                if a > b:
                    values += [((7-i)*a+i*b)//7 for i in range(1,7)]
                else:
                    values += [((5-i)*a+i*b)//5 for i in range(1,5)] + [0,255]
                alpha = [values[(selectors>>(3*i))&7] for i in range(16)]
            c0,c1,selectors = rd.unpack('<HHI')
            palette = [rgb565(c0),rgb565(c1)]
            if kind != 'DXT1' or c0 > c1:
                palette += [[(2*palette[0][k]+palette[1][k])//3 for k in range(3)]+[255],
                            [(palette[0][k]+2*palette[1][k])//3 for k in range(3)]+[255]]
            else:
                palette += [[(palette[0][k]+palette[1][k])//2 for k in range(3)]+[255],
                            [0,0,0,0 if has_alpha else 255]]
            for i in range(16):
                x,y = bx+i%4,by+i//4
                if x >= width or y >= height:
                    continue
                color = palette[(selectors>>(2*i))&3].copy()
                if kind != 'DXT1':
                    color[3] = alpha[i]
                offset = (y*width+x)*4
                out[offset:offset+4] = bytes(color)
    return bytes(out)


def decode_bgra(data,width,height,alpha):
    if not 0 < width <= 4096 or not 0 < height <= 4096 or len(data) != width*height*4:
        raise ValueError('Invalid BGRA texture size')
    out = bytearray(len(data))
    for offset in range(0,len(data),4):
        b,g,r,a = data[offset:offset+4]
        out[offset:offset+4] = bytes((r,g,b,a if alpha else 255))
    return bytes(out)


def decode_native_txd(data,wanted=None):
    dictionary = root(data,22)
    count,device = Reader(one(dictionary,1)).unpack('<HH')
    native = [body for tag,body,_ in chunks(dictionary) if tag == 21]
    if len(native) != count or not 0 < count <= 256:
        raise ValueError('Invalid native dictionary count')
    result,seen = {},set()
    for body in native:
        rd = Reader(one(body,1))
        platform,filtering = rd.unpack('<II')
        name,mask = cstring(rd.take(32)),cstring(rd.take(32))
        raster,format_id = rd.unpack('<II')
        width,height,depth,levels,typ,props = rd.unpack('<HHBBBB')
        key = name.lower()
        if key in seen:
            raise ValueError('Duplicate native texture name')
        seen.add(key)
        # Unused textures need not be decoded. Chunk bounds remain enforced.
        if wanted is not None and key not in wanted:
            continue
        fmt = struct.pack('<I',format_id).decode('ascii') if format_id in (827611204,861165636,894720068) else format_id
        if platform != 9 or raster & 0x6000 or props & 6 or fmt not in ('DXT1','DXT3','DXT5',21,22):
            raise ValueError(f'Unsupported native texture {name}: platform={platform}, format={format_id}')
        if not 0 < width <= 4096 or not 0 < height <= 4096 or not 1 <= levels <= 13:
            raise ValueError('Invalid native texture dimensions/mips')
        if fmt in (21,22) and depth != 32:
            raise ValueError('Unsupported uncompressed pixel depth')
        rgba = None
        for level in range(levels):
            w,h = max(1,width>>level),max(1,height>>level)
            size = rd.unpack('<I')[0]
            pixels = rd.take(size)
            expected = w*h*4 if fmt in (21,22) else ((w+3)//4)*((h+3)//4)*(8 if fmt=='DXT1' else 16)
            if size != expected:
                raise ValueError('Invalid native mip length')
            if level == 0:
                rgba = decode_bgra(pixels,w,h,fmt==21) if fmt in (21,22) else decode_blocks(pixels,w,h,fmt,bool(props & 1))
        if rd.pos != len(rd.data):
            raise ValueError('Invalid native texture tail')
        result[key] = dict(name=name,width=width,height=height,rgba=rgba,
                           format=str(fmt),levels=levels,filtering=filtering,
                           has_alpha=any(rgba[i]!=255 for i in range(3,len(rgba),4)),
                           smooth_alpha=any(0<rgba[i]<255 for i in range(3,len(rgba),4)))
    return result
