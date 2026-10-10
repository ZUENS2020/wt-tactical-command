"""Enumerate distinct map_obj (type, icon, color, blink) and map_info combos across all sessions.
Usage: python3 squadping/enumerate.py <data_root> [out.json]"""
import collections, glob, gzip, json, os, sys, datetime as dt

def recs(path):
    try:
        with gzip.open(path, 'rt') as f:
            for line in f:
                try: yield json.loads(line)
                except Exception: pass
    except EOFError:
        pass

def body(r):
    b = r.get('body') if 'body' in r else r.get('data')
    if isinstance(b, str):
        try: b = json.loads(b)
        except Exception: return None
    return b

def main(root, out=None):
    combos = {}; mi = collections.Counter()
    for sd in sorted(glob.glob(os.path.join(root, '*', '*'))):
        if not os.path.isdir(sd): continue
        sess = os.path.basename(sd)
        for p in sorted(glob.glob(os.path.join(sd, 'samples-*.jsonl.gz'))):
            for r in recs(p):
                ep = r.get('ep') or r.get('endpoint') or ''
                t = r.get('wall_ms') or 0
                b = body(r)
                if 'map_obj' in ep and isinstance(b, list):
                    per = collections.Counter()
                    for o in b:
                        k = (o.get('type'), o.get('icon'), o.get('color'), o.get('blink'))
                        per[k] += 1
                        c = combos.setdefault(k, dict(n=0, sess=collections.Counter(), first=t, last=t, maxper=0))
                        c['n'] += 1; c['sess'][sess] += 1
                        c['first'] = min(c['first'], t); c['last'] = max(c['last'], t)
                    for k, n in per.items():
                        combos[k]['maxper'] = max(combos[k]['maxper'], n)
                elif 'map_info' in ep and isinstance(b, dict):
                    mi[(b.get('valid'), b.get('hud_type'), tuple(b.get('grid_steps') or []))] += 1
    f = lambda ms: dt.datetime.fromtimestamp(ms/1000).strftime('%H:%M:%S') if ms else '-'
    rows = sorted(combos.items(), key=lambda kv: -kv[1]['n'])
    print('type | icon | color | blink | n | max/frame | sessions | first-last (UTC+8)')
    for k, c in rows:
        print(' | '.join(map(str, k)), '|', c['n'], '|', c['maxper'], '|', ','.join(s[-4:] for s in c['sess']), '|', f(c['first']), '-', f(c['last']))
    print('\nmap_info (valid, hud_type, grid_steps):')
    for k, n in mi.most_common(): print(k, n)
    if out:
        json.dump([dict(type=k[0], icon=k[1], color=k[2], blink=k[3], n=c['n'], maxper=c['maxper'], sessions=dict(c['sess']), first=c['first'], last=c['last']) for k, c in rows], open(out, 'w'), indent=1)

if __name__ == '__main__':
    main(*sys.argv[1:])
