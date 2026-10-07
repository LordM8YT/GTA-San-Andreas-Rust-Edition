"""Generate a simple original-free drivable car resource."""
from pathlib import Path
import json
from generate_native_room import build

boxes = [
    ((-.9,-2,-.2),(.9,2,.4),(35,140,185,255)),
    ((-.75,-.9,.4),(.75,1.1,1.15),(45,65,85,255)),
    ((-.8,-.8,1.15),(.8,1,1.25),(35,140,185,255)),
    ((-.8,1.95,.15),(-.35,2.02,.35),(245,240,195,255)),
    ((.35,1.95,.15),(.8,2.02,.35),(245,240,195,255)),
]
for x in [-1.05,.8]:
    for y in [-1.5,1.1]:
        boxes.append(((x,y,-.55),(x+.25,y+.5,.1),(30,30,35,255)))

if __name__ == '__main__':
    folder = Path(__file__).resolve().parents[1] / 'mods/native-car-demo'
    folder.mkdir(exist_ok=True)
    (folder / 'car.dff').write_bytes(build(boxes))
    (folder / 'mod.json').write_text(json.dumps({
        'schema_version': 2, 'enabled': False, 'name': 'Native custom car demo',
        'vehicles': [{'dff': 'car.dff'}]
    }, indent=2) + '\n', encoding='utf-8')
