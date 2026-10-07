"""Generate own skinned jacket/hat that share our demo player's skeleton."""
from pathlib import Path
import json
from generate_native_ped import dff, ifp

red, gold = (190,50,40,255), (225,180,50,255)
clothes = [
    ((-.275,-.16,.86),(.275,.16,1.42),red,2),
    ((-.45,-.13,.94),(-.26,.13,1.36),red,4),
    ((.26,-.13,.94),(.45,.13,1.36),red,5),
    ((-.18,-.17,1.8),(.18,.17,1.91),gold,3),
]
if __name__ == '__main__':
    folder = Path(__file__).resolve().parents[1]/'mods/native-clothing-demo'
    folder.mkdir(exist_ok=True)
    (folder/'ped.dff').write_bytes(dff())
    (folder/'ped.ifp').write_bytes(ifp())
    (folder/'clothes.dff').write_bytes(dff(clothes))
    (folder/'jacket.dff').write_bytes(dff(clothes[:3]))
    (folder/'hat.dff').write_bytes(dff(clothes[3:]))
    (folder/'mod.json').write_text(json.dumps({
        'schema_version':2,'enabled':False,'name':'Native skinned clothing demo',
        'player':{'dff':'ped.dff','ifp':'ped.ifp','clothes':[
            {'dff':'jacket.dff','name':'Rød jakke'}, {'dff':'hat.dff','name':'Gul hatt'}]}
    },indent=2)+'\n',encoding='utf-8')
