import json,urllib.request,time,sys
t0=time.time(); prev={}
for i in range(int(sys.argv[1])):
    try:
        d=json.load(urllib.request.urlopen("http://127.0.0.1:18199/api/status",timeout=3))
        row=[]
        for s in d['live_sessions']:
            n=s['records']; r=(n-prev.get(s['session'],(n,0))[0])/5
            prev[s['session']]=(n,0)
            row.append(f"{s['session'][-16:]} rec={n} +{r:.0f}/s lag={s.get('data_lag_s') or 0:.1f}s")
        print(f"t={time.time()-t0:5.0f}s  "+" | ".join(row),flush=True)
    except Exception as e: print("err",e)
    time.sleep(5)
