"""Run the enemy module over every recorded session; emit enemy_results.json + PNGs.
Usage: .venv/bin/python analyze_enemy.py"""
import sys, os, glob, gzip, json, zlib, math, statistics, collections, datetime
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from enemy import stream, reveal
from enemy.tracker import EnemyTracker
from enemy.summary import threat_summary
from enemy.classify import is_enemy_rgb, is_naval, find_player
from enemy.geo import ang_diff, heading_from_dir, SECTORS, sector_index

D = '/workspace/wt-collect/data/2026-10-10/'
OUT = '/workspace/wt-analysis/'


def ts(ms):
    return datetime.datetime.fromtimestamp(ms / 1000).strftime('%H:%M:%S.%f')[:-3]


def load(sess):
    out = []
    for f in sorted(glob.glob(D + sess + '/samples-*.jsonl.gz')):
        raw = open(f, 'rb').read()
        txt = b''
        while raw:
            d = zlib.decompressobj(31)
            try:
                txt += d.decompress(raw)
            except zlib.error:
                break
            raw = d.unused_data
        for line in txt.decode('utf8', 'replace').split('\n'):
            line = line.strip()
            if line:
                try:
                    out.append(json.loads(line))
                except ValueError:
                    pass   # last line of a live file may be partial
    out.sort(key=lambda r: (r['wall_ms'], r['seq']))
    return out


def segments(recs):
    """map_info valid runs, padded to include Player frames that arrive before valid=true."""
    segs, cur = [], None
    for r in recs:
        if not r.get('ep', '').startswith('/map_info'):
            continue
        j = stream.parse_body(r)
        v = bool(j and j.get('valid'))
        if v and cur is None:
            cur = [r['wall_ms'], r['wall_ms']]
        elif v:
            cur[1] = r['wall_ms']
        elif not v and cur is not None and j is not None:
            cur[1] = r['wall_ms']; segs.append(cur); cur = None
    if cur:
        segs.append(cur)
    return [(a - 3000, b + 1500) for a, b in segs]


def variant_key(o):
    return (o.type, o.icon, o.color, list(o.rgb), o.blink, o.has_dir, o.icon_bg)


def run_segment(sess, recs, a, b):
    sub = [r for r in recs if a <= r['wall_ms'] < b]
    tr = EnemyTracker()
    frames, huds, infos = [], [], []
    summaries = []
    variants = collections.OrderedDict()
    speed_blips = []
    last_ind = None
    for r in sub:   # indicators: speed -1 blips (possible recoil / shot)
        if r['ep'] == '/indicators':
            j = stream.parse_body(r)
            if j and j.get('army') == 'tank':
                sp = j.get('speed')
                if last_ind is not None and sp is not None and sp < 0 <= last_ind:
                    speed_blips.append(r['wall_ms'])
                last_ind = sp
    for kind, t, v in stream.events(sub):
        if kind == 'map_info':
            tr.set_map_info(v); infos.append(v)
        elif kind == 'hud':
            tr.add_hud_event(v); huds.append(v)
        else:
            frames.append(v)
            live = tr.update(v)
            for o in v.objects:
                if o.icon == 'Player' or (is_enemy_rgb(o.rgb) and not is_naval(o.icon)):
                    k = json.dumps(variant_key(o), ensure_ascii=False)
                    variants.setdefault(k, {'obs': 0, 'frames': set()})
                    variants[k]['obs'] += 1; variants[k]['frames'].add(t)
            if tr.map_info:
                s = threat_summary(live, t)
                summaries.append(s)
    kind, reasons = stream.detect_session_kind(frames, huds)
    tr.session_kind = kind
    tr.finish()
    for c in tr.all_contacts():
        reveal.classify(c, tr, final=True)
    mi = tr.map_info
    # player heading series
    pl = []
    for f in frames:
        p = find_player(f.objects)
        if p:
            pl.append((f.t_ms, p.x, p.y, heading_from_dir(p.dx, p.dy)))

    def player_at(t):
        best = None
        for row in pl:
            if row[0] <= t:
                best = row
            else:
                break
        return best

    contacts = []
    for c in tr.all_contacts():
        trans = []
        for i, (t, bl, col) in enumerate(c.blink_seq):
            end = c.blink_seq[i + 1][0] if i + 1 < len(c.blink_seq) else c.last_seen_ms
            p = player_at(t)
            relb = None
            if p and p[3] is not None:
                from enemy.geo import rel
                _, brg = rel(mi, p[1], p[2], c.history[0].x, c.history[0].y)
                relb = round(((brg - p[3] + 180) % 360) - 180, 1)
            trans.append({'t': ts(t), 't_ms': t, 'blink': bl, 'color': col,
                          'dur_s': round((end - t) / 1000, 2), 'open_until_end': i + 1 == len(c.blink_seq),
                          'rel_bearing_to_hull_deg': relb})
        hud_near_end = [e.id for e in huds if abs(e.t_ms - c.last_seen_ms) < 1500 and e.kind == 'kill']
        contacts.append({
            'contact_id': c.contact_id, 'type': c.type, 'icon': c.icon, 'cls': c.cls,
            'first_seen': ts(c.first_seen_ms), 'last_seen': ts(c.last_seen_ms),
            'lifetime_s': round((c.last_seen_ms - c.first_seen_ms) / 1000, 2),
            'spans': [(ts(s), round((e - s) / 1000, 2)) for s, e in c.visible_spans],
            'n_obs': c.n_obs, 'n_pos_changes': c.n_pos_changes,
            'pos_change_ratio': round(c.n_pos_changes / max(1, c.n_obs - 1), 3),
            'max_static_s': round(c.max_static_ms / 1000, 2),
            'speed_mps': c.speed_mps and round(c.speed_mps, 2), 'course_deg': c.course_deg and round(c.course_deg, 1),
            'heading_deg': c.heading_deg and round(c.heading_deg, 1),
            'last_norm': (round(c.x, 6), round(c.y, 6)), 'last_world': (round(c.wx, 1), round(c.wy, 1)),
            'distance_m': c.distance_m and round(c.distance_m, 1), 'bearing_deg': c.bearing_deg and round(c.bearing_deg, 1),
            'blink_timeline': trans,
            'disappeared_before_segment_end': c.last_seen_ms < max(f.t_ms for f in frames) - 500,
            'kill_hud_near_disappearance': hud_near_end,
            'reveal_kind': c.reveal_kind, 'reveal_rule': c.reveal_rule, 'reveal_confidence': c.reveal_confidence,
            '_hist': [(o.t_ms, o.x, o.y, o.blink) for o in c.history],
        })
    var_out = []
    for k, v in variants.items():
        key = json.loads(k)
        cids = [c for c in contacts if c['type'] == key[0] and c['icon'] == key[1] and
                any(tt['color'] == key[2] and tt['blink'] == key[4] for tt in c['blink_timeline'])]
        var_out.append({'type': key[0], 'icon': key[1], 'color': key[2], 'rgb': key[3], 'blink': key[4],
                        'has_dxdy': key[5], 'icon_bg': key[6], 'obs': v['obs'], 'frames': len(v['frames']),
                        'tracks': len(cids)})
    # sample of the threat summary
    sm = [s for s in summaries if s.n_visible]
    mid = sm[len(sm) // 2] if sm else None
    return {
        'session': sess, 'start': ts(a + 3000), 'end': ts(b - 1500),
        'kind': kind, 'kind_reasons': reasons,
        'map_info': mi and {'map_min': mi.map_min, 'map_max': mi.map_max, 'gen': mi.map_generation, 'hud_type': mi.hud_type},
        'n_frames': len(frames),
        'hud_live': [{'t': ts(e.t_ms), 'id': e.id, 'msg': e.msg, 'kind': e.kind, 'player': e.involves_player} for e in huds],
        'speed_minus1_blips': [ts(t) for t in speed_blips],
        'variants': sorted(var_out, key=lambda v: -v['obs']),
        'contacts': contacts,
        'threat_summary_sample': mid and {**mid.__dict__, 't': ts(mid.t_ms)},
        'player_track': pl,
    }


def main():
    results = []
    for sd in sorted(glob.glob(D + '*/')):
        sess = sd.rstrip('/').split('/')[-1]
        recs = load(sess)
        for a, b in segments(recs):
            seg = run_segment(sess, recs, a, b)
            if seg['n_frames']:
                results.append(seg)
    slim = [{**{k: v for k, v in s.items() if k not in ('player_track', 'contacts')},
             'contacts': [{k: v for k, v in c.items() if k != '_hist'} for c in s['contacts']]}
            for s in results]
    json.dump(slim, open(OUT + 'enemy_results.json', 'w'), ensure_ascii=False, indent=1, default=str)
    import pickle
    pickle.dump(results, open('/tmp/enemy_results_full.pkl', 'wb'))
    for s in slim:
        print('==', s['session'], s['start'], s['end'], s['kind'], s['kind_reasons'], 'frames', s['n_frames'])
        for c in s['contacts']:
            print('  #%d %s %s life=%.1fs pos%%=%.2f blink=%s gone_early=%s kill=%s -> %s (%s)' % (
                c['contact_id'], c['type'], c['icon'], c['lifetime_s'], c['pos_change_ratio'],
                [(t['t'][3:], t['blink'], t['color'], t['dur_s'], t['rel_bearing_to_hull_deg']) for t in c['blink_timeline']],
                c['disappeared_before_segment_end'], c['kill_hud_near_disappearance'], c['reveal_kind'], c['reveal_confidence']))
    return results


if __name__ == '__main__':
    main()
