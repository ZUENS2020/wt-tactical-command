import sys, hashlib, io, json, pickle, math, os
sys.path.insert(0,'/workspace/wt-research/WarThunder-Plotter')
import map_hashes as MH
from config import HASH_SETTINGS
from PIL import Image, ImageDraw
import numpy as np
D='/workspace/wt-collect/data/2026-10-10/'
imgs={'ground':D+'20261010-182921-shengyuan-b6a7/images/3a7d46abe5a817839c04c68c0dc7ce2578db441bea1ba0047a907dd794c8c33e.jpg',
      'air':D+'20261010-182921-shengyuan-b6a7/images/7e6edd02c5ec91acd42917c3c60c4e588cd8823080860e86929aceae370bc45f.jpg',
      'placeholder':D+'20261010-182921-shengyuan-b6a7/images/cfb99c04947c94c19d4c523c1e0fb3a2d71d403263fefcad5f4c4fc8485d07d0.png'}
def dhash(data,size=16):
    img=Image.open(io.BytesIO(data)).convert('L').resize((size,size-1),Image.Resampling.LANCZOS)
    px=list(img.getdata()); bits=[]
    for y in range(size-1):
        for x in range(size-1): bits.append(1 if px[y*size+x]>px[y*size+x+1] else 0)
    h=''
    for i in range(0,len(bits),4):
        nb=bits[i:i+4]+[0]*(4-len(bits[i:i+4])); h+=format((nb[0]<<3)|(nb[1]<<2)|(nb[2]<<1)|nb[3],'x')
    return h
out={}
print('tolerance',HASH_SETTINGS.tolerance, 'known',len(MH.ALL_MAPS))
for k,p in imgs.items():
    data=open(p,'rb').read()
    im=Image.open(io.BytesIO(data))
    h=dhash(data)
    dists=sorted(((MH._hamming_distance(h,kh),v.map_id,v.battle_type.value) for kh,v in MH.ALL_MAPS.items()))[:5]
    info=MH.lookup_map_info(h)
    a=np.asarray(im.convert('RGB')); 
    vals,counts=np.unique(a.reshape(-1,3),axis=0,return_counts=True)
    top=vals[counts.argmax()]; frac=counts.max()/counts.sum()
    out[k]={'file':os.path.basename(p),'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest(),'format':im.format,'mode':im.mode,'size':im.size,
            'dhash':h,'wtp_exact':h in MH.ALL_MAPS,'wtp_lookup':(info.map_id,info.display_name,info.battle_type.value),'nearest5':dists,
            'top_color':'#%02x%02x%02x'%tuple(top),'top_frac':float(frac),'mean_rgb':[float(x) for x in a.reshape(-1,3).mean(0)],
            'jpeg_info':{kk:str(vv) for kk,vv in im.info.items() if kk in('progressive','jfif','dpi','progression')}}
    im.convert('RGB').save(f'/workspace/wt-analysis/map_{k}_{h[:0]}{hashlib.sha256(data).hexdigest()[:12]}.png')
    print(k,json.dumps(out[k],ensure_ascii=False))
json.dump(out,open('/workspace/wt-analysis/images_results.json','w'),ensure_ascii=False,indent=1)
