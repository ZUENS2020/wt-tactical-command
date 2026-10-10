"""Extract compact replay tracks from the real 8111 recording of session
20261010-182921-shengyuan-b6a7 (wt-collector 0.1.3, game API on :9222).

Input : /tmp/b6a7_segments.jsonl.gz (cut from HK receiver samples-0001.jsonl.gz,
        /var/lib/wt-receiver/data/2026-10-10/20261010-182921-shengyuan-b6a7)
Output: src/data/replay.json  — per segment: self track + enemy tracks (simple
        nearest-neighbour association keyed by (type, icon), colour-agnostic, as in
        wt-analysis/enemy/tracker.py) + static objects. Coordinates are the raw
        normalized map_obj x/y (0..1, y down). Samples are run-length encoded
        (only stored when x/y/blink/heading change; last sample repeated at end)."""
import gzip, json, math

NAVAL = {'BattleShip', 'Boat', 'Destroyer', 'HeavyCruiser', 'Cruiser', 'Ship'}
SEG = '/tmp/b6a7_segments.jsonl.gz'

def frames(lo, hi):
    out = []
    for line in gzip.open(SEG, 'rt'):
        d = json.loads(line)
        if d['ep'] != '/map_obj.json' or d.get('status') != 200 or not d.get('body'):
            continue
        if not (lo <= d['wall_ms'] <= hi):
            continue
        objs = json.loads(d['body'])
        if objs:
            out.append((d['wall_ms'], objs))
    out.sort(key=lambda x: x[0])
    return out

def hdg(o):
    if 'dx' not in o:
        return None
    return round(math.degrees(math.atan2(o['dx'], -o['dy'])) % 360, 1)

def build(lo, hi, gate):
    fr = frames(lo, hi)
    t0 = fr[0][0]
    tracks = []      # dict(id, icon, type, s=[[t,x,y,b,h]], last=(x,y), lt)
    selfs = []
    statics = {}
    for wall, objs in fr:
        t = wall - t0
        cand = []
        for o in objs:
            ic = o.get('icon')
            if ic in NAVAL:
                continue
            if o['type'] == 'airfield':
                statics.setdefault('airfield', [round(o['sx'], 5), round(o['sy'], 5), round(o['ex'], 5), round(o['ey'], 5)])
                continue
            if o['type'] == 'bombing_point':
                k = (round(o['x'], 4), round(o['y'], 4))
                statics.setdefault('bombing', {})
                statics['bombing'].setdefault(k, t)
                continue
            if ic == 'Player':
                selfs.append([t, round(o['x'], 6), round(o['y'], 6), hdg(o), o['color']])
                continue
            cand.append(o)
        # greedy association
        pairs = []
        for ci, o in enumerate(cand):
            for ti, tr in enumerate(tracks):
                if tr['icon'] != o['icon'] or tr['type'] != o['type'] or t - tr['lt'] > 1500:
                    continue
                dd = math.hypot(o['x'] - tr['last'][0], o['y'] - tr['last'][1])
                if dd < gate:
                    pairs.append((dd, ci, ti))
        pairs.sort()
        usedc, usedt = set(), set()
        for dd, ci, ti in pairs:
            if ci in usedc or ti in usedt:
                continue
            usedc.add(ci); usedt.add(ti)
            o = cand[ci]; tr = tracks[ti]
            smp = [t, round(o['x'], 5), round(o['y'], 5), o['blink'], hdg(o)]
            p = tr['s'][-1]
            if smp[1:] != p[1:]:
                tr['s'].append(smp)
            tr['last'] = (o['x'], o['y']); tr['lt'] = t
        for ci, o in enumerate(cand):
            if ci in usedc:
                continue
            tracks.append({'id': len(tracks) + 1, 'icon': o['icon'], 'type': o['type'],
                           's': [[t, round(o['x'], 5), round(o['y'], 5), o['blink'], hdg(o)]],
                           'last': (o['x'], o['y']), 'lt': t})
    for tr in tracks:
        last = tr['s'][-1]
        if tr['lt'] != last[0]:
            tr['s'].append([tr['lt']] + last[1:])
        tr['end'] = tr['lt']
        del tr['last'], tr['lt']
    bomb = [[k[0], k[1], v] for k, v in statics.get('bombing', {}).items()]
    return {'t0': t0, 'dur': fr[-1][0] - t0, 'frames': len(fr), 'self': selfs, 'tracks': tracks,
            'airfield': statics.get('airfield'), 'bombing': bomb}

g = build(1791628296000, 1791628319000, 0.01)   # 18:31:36–18:31:59 ground test drive (Tiger H1)
a = build(1791628328000, 1791628350000, 0.006)  # 18:32:08–18:32:30 air test flight (IA-58A, Denmark)
res = {'source': 'b6a7 2026-10-10 18:31:36–18:32:30 (UTC+8), wt-collector 0.1.3 @ :9222', 'ground': g, 'air': a}
json.dump(res, open('src/data/replay.json', 'w'), separators=(',', ':'))
for k in ('ground', 'air'):
    r = res[k]
    print(k, r['frames'], 'frames', r['dur'], 'ms; self', len(r['self']), '; tracks', len(r['tracks']),
          [(t['icon'], len(t['s']), t['s'][0][0], t['end']) for t in r['tracks']][:50], 'bomb', len(r['bombing']))
