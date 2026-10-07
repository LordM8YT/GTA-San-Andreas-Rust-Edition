"""Original-free skinned humanoid and three ANP3 locomotion clips."""
from pathlib import Path
import json
import math
import struct
from generate_native_room import build, chunk

def sections(data):
    offset = 0
    while offset < len(data):
        tag, size, version = struct.unpack_from('<III', data, offset)
        yield tag, data[offset+12:offset+12+size]
        offset += 12+size

# id, parent frame, local translation
bones = [(0,-1,(0,0,0)), (1,0,(0,0,.85)), (2,1,(0,0,.3)),
         (5,2,(0,0,.5)), (32,2,(-.32,0,.2)), (22,2,(.32,0,.2)),
         (41,1,(-.16,0,0)), (51,1,(.16,0,0))]
blue, dark, skin = (40,130,195,255), (40,45,55,255), (210,165,120,255)
# Box vertices stay in model coordinates, weighted to the indicated bone.
parts = [
    ((-.25,-.14,.85),(.25,.14,1.4),blue,2),
    ((-.15,-.14,1.45),(.15,.14,1.8),skin,3),
    ((-.43,-.11,.94),(-.27,.11,1.35),blue,4),
    ((.27,-.11,.94),(.43,.11,1.35),blue,5),
    ((-.43,-.11,.84),(-.27,.11,.94),skin,4),
    ((.27,-.11,.84),(.43,.11,.94),skin,5),
    ((-.25,-.12,.1),(-.06,.12,.85),dark,6),
    ((.06,-.12,.1),(.25,.12,.85),dark,7),
    ((-.25,-.15,0),(-.06,.23,.1),dark,6),
    ((.06,-.15,0),(.25,.23,.1),dark,7),
]

def dff(custom_parts=None):
    model_parts = parts if custom_parts is None else custom_parts
    original = build([(lo,hi,color) for lo,hi,color,bone in model_parts])
    clump = dict(sections(original))[16]
    global_positions = []
    frame_data = struct.pack('<I',len(bones))
    extensions = b''
    for index,(bone,parent,position) in enumerate(bones):
        base = (0,0,0) if parent < 0 else global_positions[parent]
        global_positions.append(tuple(a+b for a,b in zip(base,position)))
        frame_data += struct.pack('<12fiI',1,0,0,0,1,0,0,0,1,*position,parent,0)
        anim = struct.pack('<3I',0x100,bone,len(bones) if index==0 else 0)
        if index == 0:
            anim += struct.pack('<2I',0,36)
            for i,(bone,_,_) in enumerate(bones):
                anim += struct.pack('<3I',bone,i,0)
        extensions += chunk(3,chunk(0x11e,anim))
    frames = chunk(1,frame_data)+extensions
    geometry_list = dict(sections(clump))[26]
    geometry = dict(sections(geometry_list))[15]
    weights = bytes([len(bones),len(bones),1,0])+bytes(range(len(bones)))
    for lo,hi,color,bone in model_parts:
        weights += bytes([bone,0,0,0])*8
    weights += struct.pack('<4f',1,0,0,0)*(len(model_parts)*8)
    for x,y,z in global_positions:
        weights += struct.pack('<16f',1,0,0,0,0,1,0,0,0,0,1,0,-x,-y,-z,1)
    weights += bytes(12)
    geometry += chunk(3,chunk(0x116,weights))
    geometry_list = chunk(1,struct.pack('<I',1))+chunk(15,geometry)
    result = b''
    for tag,body in sections(clump):
        result += chunk(tag,frames if tag==14 else geometry_list if tag==26 else body)
    return chunk(16,result)

def name(value):
    return value.encode('ascii').ljust(24,b'\0')

def ifp():
    body = name('native_demo')+struct.pack('<I',3)
    for clip,ticks,amplitude in [('idle_stance',90,0),('walk_player',72,.45),('run_player',44,.75)]:
        frames = 25
        size = frames*(16+10*(len(bones)-1))
        body += name(clip)+struct.pack('<3I',len(bones),size,1)
        for index,(bone,parent,position) in enumerate(bones):
            body += name('bone'+str(bone))+struct.pack('<3I',4 if index==0 else 3,frames,bone)
            for f in range(frames):
                cycle = math.sin(f/(frames-1)*math.tau)
                angle = amplitude*cycle*(1 if bone in [41,22] else -1 if bone in [51,32] else 0)
                body += struct.pack('<5h',round(math.sin(angle/2)*4096),0,0,
                                    round(math.cos(angle/2)*4096),round(f*ticks/(frames-1)))
                if index==0:
                    body += struct.pack('<3h',0,0,round(abs(cycle)*.015*1024))
    return b'ANP3'+struct.pack('<I',len(body))+body

if __name__ == '__main__':
    folder = Path(__file__).resolve().parents[1]/'mods/native-ped-demo'
    folder.mkdir(exist_ok=True)
    (folder/'ped.dff').write_bytes(dff())
    (folder/'ped.ifp').write_bytes(ifp())
    (folder/'mod.json').write_text(json.dumps({
        'schema_version':2,'enabled':False,'name':'Native custom player demo',
        'player':{'dff':'ped.dff','ifp':'ped.ifp'}
    },indent=2)+'\n',encoding='utf-8')
