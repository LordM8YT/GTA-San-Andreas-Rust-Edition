"""Data-only local mods. No scripts, imports, original installation writes or remote URLs."""
import json
import math
from pathlib import Path
import struct


DEFAULT_SETTINGS = dict(camera_speed=12.0,sky_color=[.53,.72,.87],fog_distance=260.0)


def vector(value,n,lo,hi,label):
    if not isinstance(value,list) or len(value)!=n or any(type(v) not in (float,int) or not math.isfinite(v) or not lo<=v<=hi for v in value):
        raise ValueError(f'Invalid {label}')
    return value


def number(value,lo,hi,label):
    if type(value) not in (int,float) or not math.isfinite(value) or not lo<=value<=hi:
        raise ValueError(f'Invalid {label}')
    return value


def read_png(path):
    if path.stat().st_size > 16*1024*1024:
        raise ValueError('Mod PNG exceeds 16 MiB')
    data = path.read_bytes()
    if len(data)<33 or data[:8] != b'\x89PNG\r\n\x1a\n' or data[12:16] != b'IHDR':
        raise ValueError('Texture override must be PNG')
    width,height = struct.unpack_from('>II',data,16)
    if not 0<width<=4096 or not 0<height<=4096:
        raise ValueError('Mod PNG exceeds texture dimensions')
    return data,width,height


def load_mods(root,enabled=True):
    root = root.resolve()
    settings = {**DEFAULT_SETTINGS}
    textures,excluded,placements,active = {},set(),[],[]
    if not enabled or not root.exists():
        return dict(settings=settings,textures=textures,excluded=excluded,placements=placements,active=active)
    for path in sorted(root.glob('*/mod.json')):
        folder = path.parent.resolve()
        if path.is_symlink() or not path.resolve().is_relative_to(root) or not folder.is_relative_to(root):
            raise ValueError('Mod paths must stay inside mods directory')
        if path.stat().st_size>128*1024:
            raise ValueError('Mod manifest exceeds 128 KiB')
        obj = json.loads(path.read_text(encoding='utf-8'))
        if not isinstance(obj,dict) or obj.get('schema_version')!=1 or type(obj.get('enabled',True)) is not bool:
            raise ValueError(f'Invalid mod manifest: {path.parent.name}')
        if not obj.get('enabled',True):
            continue
        if set(obj)-{'schema_version','enabled','name','settings','texture_overrides','exclude_model_ids','placements'}:
            raise ValueError('Unknown mod fields; executable scripts are not supported')
        label = obj.get('name',path.parent.name)
        if not isinstance(label,str) or len(label)>100:
            raise ValueError('Invalid mod name')
        config = obj.get('settings',{})
        if not isinstance(config,dict) or set(config)-set(DEFAULT_SETTINGS):
            raise ValueError('Unknown mod settings')
        for key,value in config.items():
            settings[key] = vector(value,3,0,1,key) if key=='sky_color' else number(value,1,1000,key)
        overrides = obj.get('texture_overrides',{})
        if not isinstance(overrides,dict) or len(overrides)>256:
            raise ValueError('Invalid texture overrides')
        for key,relative in overrides.items():
            if not isinstance(key,str) or len(key)>100 or key.count(':')!=1 or not isinstance(relative,str):
                raise ValueError('Use dictionary:texture keys and relative PNG paths')
            target = folder/relative
            resolved = target.resolve(strict=True)
            if Path(relative).is_absolute() or target.is_symlink() or not resolved.is_relative_to(folder):
                raise ValueError('Texture override path escapes mod folder')
            textures[key.lower()] = read_png(resolved)
        ids = obj.get('exclude_model_ids',[])
        if not isinstance(ids,list) or len(ids)>1000 or any(type(i) is not int or not 0<=i<=100000 for i in ids):
            raise ValueError('Invalid excluded model IDs')
        excluded.update(ids)
        extra = obj.get('placements',[])
        if not isinstance(extra,list) or len(extra)>100:
            raise ValueError('Invalid mod placement list')
        for item in extra:
            if not isinstance(item,dict) or set(item)-{'model_id','position','rotation'} or type(item.get('model_id')) is not int:
                raise ValueError('Invalid mod placement')
            position = vector(item.get('position'),3,-10000,10000,'mod position')
            rotation = vector(item.get('rotation',[0,0,0,1]),4,-1,1,'mod rotation')
            if not .98<=sum(v*v for v in rotation)<=1.02:
                raise ValueError('Mod rotation must be a unit quaternion')
            placements.append(dict(id=item['model_id'],position=position,rotation=rotation,source='mod:'+label))
        active.append(label)
    if len(placements)>200 or len(textures)>256:
        raise ValueError('Combined mods exceed PoC limits')
    return dict(settings=settings,textures=textures,excluded=excluded,placements=placements,active=active)
