"""Generate our own small walkable DFF room; contains no game assets."""
from pathlib import Path
import json
import struct

def chunk(tag, body):
    return struct.pack('<III', tag, len(body), 0x1803FFFF) + body

def build(custom_boxes=None):
    vertices, colors, faces = [], [], []
    def box(lo, hi, color):
        start = len(vertices)
        vertices.extend(tuple(hi[a] if n & (1 << a) else lo[a] for a in range(3)) for n in range(8))
        colors.extend([color] * 8)
        for a, b, c, d in [(0,1,3,2),(4,6,7,5),(0,4,5,1),(2,3,7,6),(0,2,6,4),(1,5,7,3)]:
            faces.extend([(start+a,start+b,start+c),(start+a,start+c,start+d)])
    box((-4,-4,-.1),(4,4,.1),(170,170,165,255))
    box((-4,-4,0),(-3.8,4,3.3),(195,155,110,255))
    box((3.8,-4,0),(4,4,3.3),(195,155,110,255))
    box((-4,3.8,0),(4,4,3.3),(160,185,195,255))
    box((-4,-4,0),(-.95,-3.8,3.3),(195,155,110,255))
    box((.95,-4,0),(4,-3.8,3.3),(195,155,110,255))
    box((-.95,-4,2.4),(.95,-3.8,3.3),(195,155,110,255))
    box((-4,-4,3.3),(4,4,3.45),(120,145,155,255))
    box((1,1,.1),(2.8,2.2,.8),(80,120,145,255))
    if custom_boxes is not None:
        vertices.clear()
        colors.clear()
        faces.clear()
        for lo, hi, color in custom_boxes:
            box(lo, hi, color)
    material=chunk(7,chunk(1,struct.pack('<I4BII3f',0,255,255,255,255,0,0,1,1,1)))
    materials=chunk(8,chunk(1,struct.pack('<Ii',1,-1))+material)
    body=struct.pack('<4I',8,len(faces),len(vertices),1)
    body+=b''.join(bytes(c) for c in colors)
    body+=b''.join(struct.pack('<4H',b,a,0,c) for a,b,c in faces)
    body+=struct.pack('<4fII',0,0,0,10,1,0)
    body+=b''.join(struct.pack('<3f',*v) for v in vertices)
    geometry=chunk(15,chunk(1,body)+materials)
    transform=struct.pack('<I12fiI',1,1,0,0,0,1,0,0,0,1,0,0,0,-1,0)
    frames=chunk(14,chunk(1,transform)+chunk(3,b''))
    geometry_list=chunk(26,chunk(1,struct.pack('<I',1))+geometry)
    atomic=chunk(20,chunk(1,struct.pack('<4I',0,0,4,0))+chunk(3,b''))
    return chunk(16,chunk(1,struct.pack('<3I',1,0,0))+frames+geometry_list+atomic+chunk(3,b''))

if __name__ == '__main__':
    folder=Path(__file__).resolve().parents[1]/'mods/native-room-demo'
    folder.mkdir(exist_ok=True)
    (folder/'room.dff').write_bytes(build())
    manifest={'schema_version':2,'enabled':False,'name':'Native walkable room demo',
              'models':[{'id':30000,'dff':'room.dff'}],
              'placements':[{'model_id':30000,'position':[2500,-1670,12.35]}]}
    (folder/'mod.json').write_text(json.dumps(manifest,indent=2)+'\n',encoding='utf-8')
