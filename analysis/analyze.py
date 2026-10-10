import sys, json, math, collections, datetime, hashlib, gzip, io, os
sys.path.insert(0,'/workspace/wt-analysis')
from load import *
import numpy as np
from PIL import Image
OUT='/workspace/wt-analysis'
def t(ms): return datetime.datetime.fromtimestamp(ms/1000).strftime('%H:%M:%S.%f')[:-3]
def pct(a,p): return float(np.percentile(a,p)) if len(a) else float('nan')
def wrap(d): return (d+180)%360-180
res={}

S2='20261010-182921-shengyuan-b6a7'; S1='20261010-180048-shengyuan-962f'
R=load(S2)
res['snapshot']={'records':len(R),'first':t(R[0]['wall_ms']),'last':t(R[-1]['wall_ms'])}
# own body-change computation
prev={}
for r in R:
    if r['ep'].startswith('#'): continue
    ok = r['status']==200
    b = r.get('img_hash') if r['ep'].startswith('/map.img') else body(r)
    r['_ok']=ok
    if ok:
        r['_new'] = (prev.get(r['ep']) != b)
        prev[r['ep']]=b
    else: r['_new']=False
    r['_j']= js(r) if ok and not r['ep'].startswith('/map.img') else None

# segments from map_info
mi=[r for r in R if r['ep']=='/map_info.json' and r['_ok']]
segs=[]; cur=None
for r in mi:
    j=r['_j'] or {}
    v=j.get('valid')
    if v:
        g=j.get('map_generation')
        if cur and cur['gen']==g: cur['last_valid']=r['wall_ms']
        else:
            cur={'gen':g,'start':r['wall_ms'],'last_valid':r['wall_ms'],'end':None}; segs.append(cur)
    else:
        if cur and cur['end'] is None: cur['end']=r['wall_ms']
        cur=None
for s in segs:
    if s['end'] is None: s['end']=R[-1]['wall_ms']
print('segments', [(s['gen'],t(s['start']),t(s['end'])) for s in segs])

def player(j):
    if not isinstance(j,list): return None
    for o in j:
        if o.get('icon')=='Player': return o
    return None

def seg_analysis(s,label):
    W=[r for r in R if s['start']<=r['wall_ms']<s['end'] and not r['ep'].startswith('#')]
    dur=(s['end']-s['start'])/1000
    out={'label':label,'gen':s['gen'],'start':t(s['start']),'end':t(s['end']),'dur_s':dur}
    # Player-present window
    mo=[r for r in W if r['ep']=='/map_obj.json' and r['_ok']]
    pres=[r for r in mo if player(r['_j'])]
    out['player_first']=t(pres[0]['wall_ms']) if pres else None
    out['player_last']=t(pres[-1]['wall_ms']) if pres else None
    out['player_types']=dict(collections.Counter(player(r['_j'])['type'] for r in pres))
    ind={r['wall_ms']//1:r for r in W if r['ep']=='/indicators' and r['_ok']}
    indl=sorted([r for r in W if r['ep']=='/indicators' and r['_ok']],key=lambda r:r['mono_ns'])
    import bisect
    indm=[r['mono_ns'] for r in indl]
    def near_ind(r,tol=60e6):
        i=bisect.bisect_left(indm,r['mono_ns'])
        best=None
        for k in (i-1,i):
            if 0<=k<len(indl) and abs(indl[k]['mono_ns']-r['mono_ns'])<tol:
                if best is None or abs(indl[k]['mono_ns']-r['mono_ns'])<abs(best['mono_ns']-r['mono_ns']): best=indl[k]
        return best
    # moving classification per tick (by mono_ns ticks via indicators speed)
    def moving(r):
        ir=near_ind(r)
        if not ir or not ir['_j']: return None
        sp=ir['_j'].get('speed')
        if sp is None: return None
        return abs(sp)>0.5
    # per endpoint stats
    eps={}
    for ep in sorted(set(r['ep'] for r in W)):
        L=sorted([r for r in W if r['ep']==ep],key=lambda r:r['mono_ns'])
        okL=[r for r in L if r['_ok']]
        lat=[r['lat_us']/1000 for r in okL]
        news=[r for r in okL if r['_new']]
        req_iv=np.diff([r['mono_ns'] for r in L])/1e6 if len(L)>1 else []
        chg_iv=np.diff([r['mono_ns'] for r in news])/1e6 if len(news)>1 else []
        d={'cfg_hz':L[0]['cfg_hz'],'n':len(L),'ok':len(okL),'err':len(L)-len(okL),
           'req_hz':len(L)/dur,'new':len(news),'new_pct':100*len(news)/max(1,len(okL)),'new_hz':len(news)/dur,
           'lat_p50':pct(lat,50),'lat_p95':pct(lat,95),'lat_p99':pct(lat,99),'lat_max':max(lat) if lat else None,
           'req_iv_p50':pct(req_iv,50),'req_iv_max':float(max(req_iv)) if len(req_iv) else None,
           'chg_iv_p50':pct(chg_iv,50),'chg_iv_p95':pct(chg_iv,95),
           'bytes':sum(r['bytes'] for r in L),'avg_bytes':float(np.mean([r['bytes'] for r in okL])) if okL else 0}
        if L[0]['cfg_hz']>=10:
            for mv in (True,False):
                sub=[r for r in okL if moving(r)==mv and (player(next((m['_j'] for m in [r]),None)) or True)]
                # restrict to player-present period
                if pres: sub=[r for r in sub if pres[0]['wall_ms']<=r['wall_ms']<=pres[-1]['wall_ms']]
                tsum=0
                if sub:
                    d['moving' if mv else 'static']={'n':len(sub),'new_pct':100*sum(r['_new'] for r in sub)/len(sub),
                        'new_hz_est':10*sum(r['_new'] for r in sub)/len(sub)*len(sub)/len(sub)}
        eps[ep]=d
        if ep in('/map_obj.json','/indicators','/state'):
            out.setdefault('chg_iv',{})[ep]=list(map(float,chg_iv))
    out['endpoints']=eps
    # real new-sample Hz while moving/static using time span
    # Player position freshness
    pos=[(r['mono_ns'],player(r['_j'])) for r in pres]
    chg_t=[]; last=None; same=0
    for m,p in pos:
        xy=(p['x'],p['y'])
        if xy!=last: chg_t.append(m); last=xy
    iv=np.diff(chg_t)/1e6
    out['player_pos']={'samples':len(pos),'changes':len(chg_t),'chg_pct':100*len(chg_t)/max(1,len(pos)),
        'iv_p50':pct(iv,50),'iv_p95':pct(iv,95),'iv_max':float(max(iv)) if len(iv) else None,'iv_min':float(min(iv)) if len(iv) else None,
        'iv_hist':dict(collections.Counter(int(round(x/50.0))*50 for x in iv))}
    out['_pos_iv']=list(map(float,iv))
    # moving vs static breakdown of player pos changes
    mvst=collections.Counter()
    for r in pres:
        mvst[(moving(r),)] +=1
    # player position change ratio while moving/static
    lastxy=None; cm=collections.Counter(); tot=collections.Counter()
    for r in pres:
        p=player(r['_j']); xy=(p['x'],p['y']); mv=moving(r)
        tot[mv]+=1
        if lastxy is not None and xy!=lastxy: cm[mv]+=1
        lastxy=xy
    out['player_pos_by_motion']={str(k):{'n':tot[k],'pos_changed':cm[k],'pct':100*cm[k]/tot[k]} for k in tot}
    # time span of moving/static to compute Hz
    # heading
    hd=[]; trk=[]
    mi_j=[r['_j'] for r in W if r['ep']=='/map_info.json' and r['_ok'] and r['_j'] and r['_j'].get('valid')]
    mn=mi_j[0]['map_min']; mx=mi_j[0]['map_max']
    span=(mx[0]-mn[0], mx[1]-mn[1])
    norms=[]
    track=[]
    for r in pres:
        p=player(r['_j'])
        track.append((r['mono_ns'],p['x'],p['y'],p.get('dx'),p.get('dy')))
        if 'dx' in p:
            norms.append(math.hypot(p['dx'],p['dy']))
            h=(math.degrees(math.atan2(p['dx'],-p['dy']))+360)%360
            ir=near_ind(r,30e6)
            if ir and ir['_j'] and 'compass' in ir['_j']:
                hd.append((h,ir['_j']['compass'],wrap(h-ir['_j']['compass']),ir['mono_ns']-r['mono_ns']))
    out['dxdy_present_pct']=100*sum(1 for r in pres if 'dx' in player(r['_j']))/max(1,len(pres))
    out['dxdy_norm']=[min(norms),max(norms)] if norms else None
    if hd:
        e=np.abs([x[2] for x in hd])
        out['heading_vs_compass']={'n':len(hd),'mean_abs':float(e.mean()),'max_abs':float(e.max()),'p95':pct(e,95),
             'same_tick_n':sum(1 for x in hd if x[3]==0)}
    # heading vs track bearing (world coords), using displacement over >=0.5s and >1 world unit
    tb=[]
    for i in range(len(track)):
        for j in range(i+1,len(track)):
            if (track[j][0]-track[i][0])>=0.5e9:
                wx=(track[j][1]-track[i][1])*span[0]; wy=(track[j][2]-track[i][2])*span[1]
                dist=math.hypot(wx,wy)
                if dist>2 and track[i][3] is not None:
                    b=(math.degrees(math.atan2(wx,-wy))+360)%360
                    hm=(math.degrees(math.atan2((track[i][3]+track[j][3])/2,-(track[i][4]+track[j][4])/2))+360)%360
                    tb.append((wrap(hm-b),dist/((track[j][0]-track[i][0])/1e9)))
                break
    if tb:
        e=np.abs([x[0] for x in tb])
        out['heading_vs_track']={'n':len(tb),'mean_abs':float(e.mean()),'median_abs':pct(e,50),'max_abs':float(e.max()),'speed_wu_s_median':pct([x[1] for x in tb],50),'reverse_like':int(sum(1 for x in e if x>135))}
    out['_track']=track; out['_span']=span; out['_mn']=mn; out['_mx']=mx
    # map_info details
    out['map_info_samples']=[ {k:v for k,v in j.items()} for j in mi_j[:1]]
    out['map_info_types']={k:type(v).__name__ if not isinstance(v,list) else 'list['+type(v[0]).__name__+']' for k,v in mi_j[0].items()}
    out['map_info_distinct']=len(set(json.dumps(j,sort_keys=True) for j in mi_j))
    # objects
    objc=collections.Counter(); dxdy=collections.Counter(); keys=collections.Counter()
    nobj=[]
    for r in mo:
        j=r['_j'] if isinstance(r['_j'],list) else []
        nobj.append(len(j))
        for o in j:
            k=(o.get('type'),o.get('icon'),o.get('color'),o.get('blink'))
            objc[k]+=1
            if 'dx' in o: dxdy[k]+=1
            keys[tuple(sorted(o.keys()))]+=1
    out['objects']=[ {'type':k[0],'icon':k[1],'color':k[2],'blink':k[3],'obs':v,'dxdy_pct':100*dxdy[k]/v} for k,v in objc.most_common()]
    out['obj_keysets']={'|'.join(k):v for k,v in keys.items()}
    out['nobj']={'min':min(nobj),'max':max(nobj),'mean':float(np.mean(nobj)),'empty':sum(1 for n in nobj if n==0)}
    out['filtered_naval']=sum(r.get('filtered_naval') or 0 for r in mo)
    out['filtered_naval_samples']=sum(1 for r in mo if r.get('filtered_naval'))
    # indicators/state keys
    for ep in ('/indicators','/state'):
        js_=[r['_j'] for r in W if r['ep']==ep and r['_ok'] and isinstance(r['_j'],dict)]
        kc=collections.Counter(k for j in js_ for k in j)
        out[ep+'_keys']={k:v for k,v in kc.items()}
        out[ep+'_valid']=dict(collections.Counter(str(j.get('valid')) for j in js_))
        out[ep+'_types']=dict(collections.Counter(j.get('type') for j in js_))
        out[ep+'_n']=len(js_)
    # speeds
    sp=[r['_j'].get('speed') for r in indl if r['_j'] and r['_j'].get('speed') is not None]
    out['ind_speed']={'min':min(sp),'max':max(sp),'moving_pct':100*sum(1 for x in sp if abs(x)>0.5)/len(sp)} if sp else None
    # mission/hudmsg/gamechat
    for ep in ('/mission.json','/hudmsg','/gamechat'):
        L=[r for r in W if r['ep']==ep and r['_ok']]
        out[ep]={'n':len(L),'distinct_bodies':len(set(body(r) for r in L)),'urls':dict(collections.Counter(r['url'] for r in L)),
                 'bodies':list(collections.Counter(body(r) for r in L).items())[:5]}
    # map.img
    out['map_img']=dict(collections.Counter((r['url'],r.get('img_hash'),r.get('ctype'),r['bytes']) for r in W if r['ep'].startswith('/map.img') and r['_ok']).items()) 
    out['map_img']={str(k):v for k,v in out['map_img'].items()}
    # bandwidth
    by=collections.Counter(); 
    for r in W: by[r['ep']]+=r['bytes']
    raw='\n'.join(json.dumps({k:v for k,v in r.items() if not k.startswith('_')},ensure_ascii=False) for r in W).encode()
    out['bytes_by_ep']=dict(by); out['bytes_total']=sum(by.values())
    out['bytes_per_h_MB']={k:v/dur*3600/1e6 for k,v in by.items()}
    out['record_json_bytes']=len(raw); out['gzip_bytes']=len(gzip.compress(raw,6))
    out['errors']=[(t(r['wall_ms']),r['ep'],r.get('err'),r['lat_us']/1000) for r in W if not r['_ok']]
    return out

labels={}
for s in segs:
    # label by player type
    pass
res['segments_raw']=[(s['gen'],t(s['start']),t(s['end'])) for s in segs]
SEG={}
for s in segs:
    a=seg_analysis(s,'?')
    lab='ground' if 'ground_model' in a['player_types'] else ('air' if 'aircraft' in a['player_types'] else 'other')
    a['label']=lab
    SEG[lab+'_gen%s'%s['gen']]=a
res['segments']={k:{kk:vv for kk,vv in v.items() if not kk.startswith('_')} for k,v in SEG.items()}

# whole-session anomalies
def anomalies(RR):
    out={}
    errs=[(t(r['wall_ms']),r['ep'],r.get('err'),r['lat_us']/1000) for r in RR if not r['ep'].startswith('#') and r['status']!=200]
    out['errors']=errs
    out['err_by_kind']=dict(collections.Counter((r.get('err') or str(r['status'])).split(':')[0] for r in RR if not r['ep'].startswith('#') and r['status']!=200))
    # gaps: 10Hz endpoints request interval >300ms
    gaps=[]
    for ep in ('/map_obj.json',):
        L=sorted([r for r in RR if r['ep']==ep],key=lambda r:r['mono_ns'])
        for a,b in zip(L,L[1:]):
            iv=(b['mono_ns']-a['mono_ns'])/1e6
            if iv>300: gaps.append((t(a['wall_ms']),t(b['wall_ms']),round(iv)))
    out['map_obj_gaps_gt300ms']=gaps
    # monotonic vs wall consistency
    L=[r for r in RR if not r['ep'].startswith('#')]
    w=np.array([r['wall_ms'] for r in L]); m=np.array([r['mono_ns'] for r in L])/1e6
    out['wall_minus_mono_drift_ms']=float((w-m).max()-(w-m).min())
    # seq gaps
    seqs=sorted(r['seq'] for r in RR)
    out['seq_range']=(seqs[0],seqs[-1],len(seqs),seqs[-1]-seqs[0]+1-len(seqs))
    # map_obj empty / map_info valid periods
    per=[];cur=None
    for r in sorted([r for r in RR if r['ep']=='/map_info.json' and r['status']==200],key=lambda r:r['mono_ns']):
        j=js(r) or {}; v=bool(j.get('valid'))
        if cur is None or cur[0]!=v: cur=[v,r['wall_ms'],r['wall_ms']]; per.append(cur)
        else: cur[2]=r['wall_ms']
    out['map_info_valid_periods']=[(v,t(a),t(b),round((b-a)/1000,1)) for v,a,b in per]
    mo=[r for r in RR if r['ep']=='/map_obj.json' and r['status']==200]
    out['map_obj_bodies']=dict(collections.Counter(body(r) if len(body(r) or '')<10 else 'nonempty' for r in mo))
    st=[js(r) for r in RR if r['ep']=='/state' and r['status']==200]
    out['state_valid']=dict(collections.Counter(str((j or {}).get('valid')) for j in st))
    # indicators in hangar
    ii=[js(r) for r in RR if r['ep']=='/indicators' and r['status']==200]
    out['ind_valid_types']=dict(collections.Counter(str(((j or {}).get('valid'),(j or {}).get('army'),(j or {}).get('type'))) for j in ii))
    out['ind_keys_hangar']=sorted(set(k for j in ii if j for k in j))[:80]
    # hudmsg urls
    hu=[r for r in RR if r['ep']=='/hudmsg']
    out['hudmsg_urls']=dict(collections.Counter(r['url'] for r in hu))
    out['gamechat_urls']=dict(collections.Counter(r['url'] for r in RR if r['ep']=='/gamechat'))
    out['mission_bodies']=dict(collections.Counter(body(r) for r in RR if r['ep']=='/mission.json' and r['status']==200))
    # hudmsg nonempty bodies
    ne=[(t(r['wall_ms']),r['url'],(body(r) or '')[:600]) for r in hu if r['status']==200 and len(body(r) or '')>40]
    out['hudmsg_nonempty']=ne[:4]; out['hudmsg_nonempty_n']=len(ne)
    span=(L[-1]['wall_ms']-L[0]['wall_ms'])/1000
    by=collections.Counter()
    for r in RR: by[r['ep']]+=r['bytes']
    out['span_s']=span
    out['bytes_per_h_MB']={k:v/span*3600/1e6 for k,v in by.items()}
    out['bytes_total']=sum(by.values())
    out['loc']=[(t(r['wall_ms']),r['url'],r['status'],body(r)) for r in RR if r['ep'].startswith('/loc')]
    return out
res['anom_S2']=anomalies(R)
R1=load(S1)
res['S1']={'records':len(R1),'first':t(R1[0]['wall_ms']),'last':t(R1[-1]['wall_ms']),'meta':body(R1[0])[:3000]}
res['anom_S1']=anomalies(R1)
# S1 latencies per endpoint
lat1={}
for ep in set(r['ep'] for r in R1 if not r['ep'].startswith('#')):
    l=[r['lat_us']/1000 for r in R1 if r['ep']==ep and r['status']==200]
    if l: lat1[ep]=(len(l),pct(l,50),pct(l,95),pct(l,99),max(l))
res['S1_lat']=lat1
json.dump(res,open(OUT+'/analysis_results.json','w'),ensure_ascii=False,indent=1,default=str)
import pickle; pickle.dump({k:{kk:vv for kk,vv in v.items()} for k,v in SEG.items()},open(OUT+'/seg.pkl','wb'))
print('done')
