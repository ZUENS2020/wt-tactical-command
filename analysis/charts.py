import pickle, json, sys, math
import numpy as np, matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
from matplotlib import font_manager
from PIL import Image
fp='/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc'
try:
    font_manager.fontManager.addfont(fp); plt.rcParams['font.family']=font_manager.FontProperties(fname=fp).get_name()
except Exception as e: print(e)
plt.rcParams['axes.unicode_minus']=False
sys.path.insert(0,'/workspace/wt-analysis')
from load import *
S=pickle.load(open('seg.pkl','rb'))
G=S['ground_gen26']; A=S['air_gen27']
# 1 histogram of body-change intervals
fig,axs=plt.subplots(1,2,figsize=(13,4.5))
for ax,(lab,s) in zip(axs,[('陆战试车 (Tiger H1, firing range)',G),('空战试飞 (IA-58A Pucará, Denmark)',A)]):
    bins=np.arange(0,600,10)
    for ep,c in [('/map_obj.json','C0'),('/indicators','C1'),('/state','C2')]:
        iv=s['chg_iv'].get(ep,[])
        if iv: ax.hist(np.clip(iv,0,595),bins=bins,alpha=.5,label=f'{ep} (n={len(iv)}, p50={np.median(iv):.0f}ms)',color=c)
    pv=s['_pos_iv']; ax.hist(np.clip(pv,0,595),bins=bins,histtype='step',lw=2,color='k',label=f'本人 x/y 变化间隔 (n={len(pv)})')
    ax.set_title(lab); ax.set_xlabel('相邻两次"内容变化"的间隔 ms（>600 归入最右）'); ax.set_ylabel('次数'); ax.legend(fontsize=8)
fig.suptitle('10 Hz 轮询下各端点内容变化间隔（实测 2026-10-10）'); fig.tight_layout(); fig.savefig('update_interval_hist.png',dpi=110)
# 2 overlays
R=load('20261010-182921-shengyuan-b6a7')
def last_objs(s):
    # map_obj sample in middle of player-present window
    lo,hi=s['_track'][0][0],s['_track'][-1][0]; mid=(lo+hi)/2
    best=None
    for r in R:
        if r['ep']=='/map_obj.json' and r['status']==200 and abs(r['mono_ns']-mid)<60e6: best=r;break
    return js(best)
def overlay(s,img,out,title,zoom):
    im=Image.open(img).convert('RGB'); W,H=im.size
    objs=last_objs(s)
    fig,axs=plt.subplots(1,2,figsize=(15,7.5))
    tr=np.array([[x[1],x[2]] for x in s['_track']])
    for ax,full in zip(axs,(True,False)):
        ax.imshow(im)
        for o in objs:
            if o.get('icon')=='Player': continue
            col=o.get('color','#fff')
            if 'sx' in o: ax.plot([o['sx']*W,o['ex']*W],[o['sy']*H,o['ey']*H],color=col,lw=3)
            else:
                ax.scatter(o['x']*W,o['y']*H,s=30 if full else 60,c=col,edgecolors='white',linewidths=.5,marker='s' if o['type']=='ground_model' else ('^' if o['type']=='aircraft' else 'D'))
        ax.plot(tr[:,0]*W,tr[:,1]*H,'-',color='yellow',lw=2)
        x0,y0,dx,dy=s['_track'][-1][1:5]
        L=(0.01 if full else zoom*0.15)*W
        ax.arrow(x0*W,y0*H,dx*L,dy*L,color='yellow',width=W*0.0015 if full else W*zoom*0.004,length_includes_head=True)
        if not full:
            cx,cy=tr.mean(0); hw=zoom/2
            ax.set_xlim((cx-hw)*W,(cx+hw)*W); ax.set_ylim((cy+hw)*H,(cy-hw)*H)
            ax.set_title(f'局部放大（视野 {zoom*s["_span"][0]:.0f} 世界单位）')
        else: ax.set_title('整图 %dx%d'%(W,H))
    fig.suptitle(title); fig.tight_layout(); fig.savefig(out,dpi=100); plt.close(fig)
D='/workspace/wt-collect/data/2026-10-10/20261010-182921-shengyuan-b6a7/images/'
overlay(G,D+'3a7d46abe5a817839c04c68c0dc7ce2578db441bea1ba0047a907dd794c8c33e.jpg','map_ground_track_overlay.png',
  '陆战试车 map.img（JPEG 2048², firing_range_tankmap）+ 本人轨迹（黄）+ 敌方目标（红）  地图 0..4096',0.06)
overlay(A,D+'7e6edd02c5ec91acd42917c3c60c4e588cd8823080860e86929aceae370bc45f.jpg','map_air_track_overlay.png',
  '空战试飞 map.img（JPEG 2048², air_denmark_map）+ 本人轨迹（黄）+ 对象  地图 ±65536',0.06)
# 3 latency CDF
fig,ax=plt.subplots(figsize=(8,4.5))
for lab,s,ls in [('陆战',G,'-'),('空战',A,'--')]:
    for ep,c in [('/map_obj.json','C0'),('/indicators','C1'),('/state','C2')]:
        l=sorted(r['lat_us']/1000 for r in R if r['ep']==ep and r['status']==200 and s['_track'][0][0]<=r['mono_ns']<=s['_track'][-1][0])
        ax.plot(l,np.linspace(0,1,len(l)),ls,color=c,label=f'{lab} {ep}')
ax.set_xscale('log'); ax.set_xlabel('响应延迟 ms (log)'); ax.set_ylabel('CDF'); ax.grid(alpha=.3); ax.legend(fontsize=8); ax.set_title('本人在场期间 8111(9222) 响应延迟 CDF')
fig.tight_layout(); fig.savefig('latency_cdf.png',dpi=110)
print('ok')
