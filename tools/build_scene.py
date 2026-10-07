"""Convert a small region into bounded draw batches in memory, without original exe."""
from collections import defaultdict
import math

from load_world import region
from static_geometry import decode_static_dff
from native_textures import decode_native_txd
from load_assets import png


def rotate_ipl(point,quaternion):
    # IPL stores the inverse world orientation. Conjugate and normalize it.
    length = math.sqrt(sum(v*v for v in quaternion))
    x,y,z,w = [-quaternion[i]/length for i in range(3)]+[quaternion[3]/length]
    px,py,pz = point
    tx,ty,tz = 2*(y*pz-z*py),2*(z*px-x*pz),2*(x*py-y*px)
    return [px+w*tx+y*tz-z*ty,py+w*ty+z*tx-x*tz,pz+w*tz+x*ty-y*tx]


def place(point,row,center):
    x,y,z = rotate_ipl(point,row['rotation'])
    x+=row['position'][0]-center[0]
    y+=row['position'][1]-center[1]
    z+=row['position'][2]
    return [x,z,-y]


def normalize(v):
    length = math.sqrt(sum(x*x for x in v)) or 1
    return [x/length for x in v]


def face_normal(a,b,c):
    u=[b[i]-a[i] for i in range(3)];v=[c[i]-a[i] for i in range(3)]
    return normalize([u[1]*v[2]-u[2]*v[1],u[2]*v[0]-u[0]*v[2],u[0]*v[1]-u[1]*v[0]])


def build_scene(game,center,radius,mods,progress=print):
    archive,rows,summary = region(game,center,radius)
    detailed = [r for r in rows if r['definition'] and not r['definition']['model'].lower().startswith('lod') and r['id'] not in mods['excluded']]
    for extra in mods['placements']:
        obj = archive.definitions.get(extra['id'])
        if not obj:
            raise ValueError(f'Mod placement ID has no static definition: {extra["id"]}')
        if math.hypot(extra['position'][0]-center[0],extra['position'][1]-center[1])>radius:
            raise ValueError('Mod placement must be inside selected region')
        detailed.append({**extra,'definition':obj,'dff':obj['model'].lower()+'.dff','txd':obj['txd'].lower()+'.txd'})
    if not detailed or len(detailed)>2000:
        raise ValueError('No detailed placements or region exceeds 2000 objects')
    progress(f'Loading {len(detailed)} detailed placements...')
    models = {};wanted = defaultdict(set)
    for r in detailed:
        if r['dff'] not in models:
            models[r['dff']] = decode_static_dff(archive.read(r['dff']))
        wanted[r['txd']].update(m['texture'] for g in models[r['dff']] for m in g['materials'] if m['texture'])
    images = {};textures = {};metadata = []
    for i,(txd,names) in enumerate(sorted(wanted.items())):
        decoded = decode_native_txd(archive.read(txd),names)
        missing = names-set(decoded)
        if missing:
            raise ValueError(f'Missing texture in {txd}: {sorted(missing)}')
        for name,t in decoded.items():
            key = txd[:-4]+':'+name
            index = len(metadata)
            url = f'/api/texture/{index}.png'
            override = mods['textures'].get(key)
            data,width,height = override if override else (png(t),t['width'],t['height'])
            images[url] = data
            textures[key] = index
            # Overrides may introduce alpha, so render them in the alpha pass.
            metadata.append(dict(key=key,url=url,width=width,height=height,has_alpha=bool(override) or t['has_alpha'],smooth_alpha=bool(override) or t['smooth_alpha'],override=bool(override),filtering=t['filtering']))
        progress(f'Textures {i+1}/{len(wanted)}',end='\r',flush=True)
    progress('')
    white = len(metadata)
    images[f'/api/texture/{white}.png'] = png(dict(width=1,height=1,rgba=b'\xff'*4))
    metadata.append(dict(key='runtime:white',url=f'/api/texture/{white}.png',width=1,height=1,has_alpha=False,smooth_alpha=False,override=False,filtering=0))
    batches = defaultdict(list);triangles=0
    bounds_min=[math.inf]*3;bounds_max=[-math.inf]*3
    for r in detailed:
        for g in models[r['dff']]:
            positions=[place(v,r,center) for v in g['vertices']]
            normals=[]
            for v in g['normals']:
                x,y,z = rotate_ipl(v,r['rotation']);normals.append(normalize([x,z,-y]))
            for a,b,c,material_id in g['triangles']:
                material=g['materials'][material_id]
                texture_id=textures[r['txd'][:-4]+':'+material['texture']] if material['texture'] else white
                color=material['color']
                alpha=metadata[texture_id]['smooth_alpha'] or color[3]<255 or any(g['colors'][v][3]<255 for v in (a,b,c))
                values=batches[(texture_id,alpha)]
                normal=face_normal(positions[a],positions[b],positions[c]) if not normals else None
                for vertex in (a,b,c):
                    position=positions[vertex]
                    for axis in range(3):
                        bounds_min[axis]=min(bounds_min[axis],position[axis]);bounds_max[axis]=max(bounds_max[axis],position[axis])
                    prelight=[g['colors'][vertex][i]*color[i]/65025 for i in range(4)]
                    values.extend([*position,*g['uvs'][vertex],*(normals[vertex] if normals else normal),*prelight])
                triangles+=1
                if triangles>300000:
                    raise ValueError('Scene exceeds 300,000 triangle PoC budget')
    batches_list=[]
    for (texture_id,alpha),values in sorted(batches.items()):
        # Draw positions determine ordering for the approximate alpha pass.
        count=len(values)//12
        centroid=[sum(values[i+axis] for i in range(0,len(values),12))/count for axis in range(3)]
        batches_list.append(dict(texture=texture_id,alpha=alpha,vertices=values,center=centroid))
    summary.update(rendered_placements=len(detailed),unique_models=len(models),triangles=triangles,textures=len(metadata)-1,draw_batches=len(batches_list),active_mods=mods['active'])
    return dict(center=list(center),radius=radius,bounds=[bounds_min,bounds_max],batches=batches_list,textures=metadata,
                summary=summary,settings=mods['settings'],spawn=[2490-center[0],15.5,-(-1665-center[1])]),images
