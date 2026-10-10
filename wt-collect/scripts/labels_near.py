#!/usr/bin/env python3
"""List label events (wt-collector 0.2.0 GUI buttons) with the enemy map markers present in map_obj
around each label (default +-3 s), plus hudmsg damage/events in the same window.

Usage:
  labels_near.py <session_dir | data_root> [--session SUBSTR] [--window 3] [--test] [--undone] [--csv out.csv] [--frames]
    session_dir : .../data/2026-10-10/<session>   (samples-*.jsonl.gz)
    data_root   : .../data  (all sessions that have labels.jsonl; filter with --session)
Times are aligned on the synced server clock (Record.ts_server_ms / label t_server), shown in UTC+8.
Enemy = map_obj object with color r>=200,g<130,b<130 (same rule as wt-analysis/enemy/classify.py), not Player.
Labels tagged "test" and undone labels are skipped unless --test / --undone.
"""
import argparse, csv, glob, gzip, json, math, os, sys
from datetime import datetime, timezone, timedelta

TZ = timezone(timedelta(hours=8))

def fmt(ms):
    return datetime.fromtimestamp(ms / 1000, TZ).strftime("%m-%d %H:%M:%S.%f")[:-3] if ms else "-"

def rgb(c):
    try:
        c = c.lstrip("#")
        return int(c[0:2], 16), int(c[2:4], 16), int(c[4:6], 16)
    except Exception:
        return (0, 0, 0)

def is_enemy(o):
    if o.get("icon") == "Player":
        return False
    r, g, b = rgb(o.get("color", ""))
    return r >= 200 and g < 130 and b < 130

def t_of(r):
    return r.get("ts_server_ms") or r.get("wall_ms")

def read_records(sdir):
    out = []
    for f in sorted(glob.glob(os.path.join(sdir, "samples-*.jsonl*"))):
        op = gzip.open if f.endswith(".gz") else open
        try:
            with op(f, "rt", encoding="utf-8") as fh:
                for l in fh:
                    l = l.strip()
                    if l:
                        try:
                            out.append(json.loads(l))
                        except Exception:
                            pass
        except EOFError:
            pass
    seen, uniq = set(), []
    for r in sorted(out, key=lambda r: r.get("seq", 0)):
        if r.get("seq") in seen:
            continue
        seen.add(r.get("seq"))
        uniq.append(r)
    return uniq

def resolve_labels(lrecs, want_test, want_undone):
    labs, undone, tags = [], set(), {}
    for r in lrecs:
        try:
            b = json.loads(r["body"])
        except Exception:
            continue
        act = b.get("action")
        if act == "undo":
            undone.add(b.get("ref_uid"))
        elif act == "tag":
            tags.setdefault(b.get("ref_uid"), set()).update(b.get("tags") or [])
        else:
            labs.append(b)
    out = []
    for b in labs:
        t = set(b.get("tags") or []) | tags.get(b.get("uid"), set())
        b["tags"] = sorted(t)
        b["undone"] = b.get("uid") in undone
        if "test" in t and not want_test:
            continue
        if b["undone"] and not want_undone:
            continue
        out.append(b)
    out.sort(key=lambda b: b.get("t_server") or b.get("t_server_synced") or b.get("t_local") or 0)
    return out

def meters(mi, a, b):
    if not mi:
        return None
    try:
        (x0, y0), (x1, y1) = mi["map_min"], mi["map_max"]
        return math.hypot((a[0] - b[0]) * (x1 - x0), (a[1] - b[1]) * (y1 - y0))
    except Exception:
        return None

def analyze(sdir, args, writer):
    recs = read_records(sdir)
    labels = resolve_labels([r for r in recs if r.get("ep") == "#label"], args.test, args.undone)
    if not labels:
        return 0
    frames = [(t_of(r), r) for r in recs if r.get("ep") == "/map_obj.json" and r.get("body") and not r.get("err")]
    infos = [(t_of(r), r) for r in recs if r.get("ep") == "/map_info.json" and r.get("body")]
    huds = [(t_of(r), r) for r in recs if r.get("ep") == "/hudmsg" and r.get("body")]
    gates = [(t_of(r), r) for r in recs if r.get("ep") in ("#state",)]
    w = int(args.window * 1000)
    print(f"\n=== {os.path.basename(sdir)}: {len(labels)} label(s), {len(frames)} map_obj frames")
    for lab in labels:
        t = lab.get("t_server") or lab.get("t_server_synced") or lab.get("t_local")
        flags = []
        if not lab.get("clock_synced"):
            flags.append("UNSYNCED(" + str(lab.get("t_server_source")) + ")")
        if lab.get("late"):
            flags.append("late")
        if lab["undone"]:
            flags.append("UNDONE")
        if lab["tags"]:
            flags.append("tags=" + ",".join(lab["tags"]))
        sn = lab.get("snapshot") or {}
        print(f"\n[{fmt(t)}] {lab.get('label_id')} {lab.get('text')}" + (f" 「{lab['note']}」" if lab.get("note") else "") +
              f"  src={lab.get('source')} {' '.join(flags)}")
        print(f"   snapshot: in_battle={sn.get('in_battle')} gen={sn.get('map_gen')} self={sn.get('self_xy')} {sn.get('self_type') or ''} "
              f"enemies={sn.get('enemy_objects')} objs={sn.get('objects')} vehicle={sn.get('vehicle')}")
        g = [r for tt, r in gates if tt <= t]
        if g:
            gb = json.loads(g[-1]["body"])
            if gb.get("paused"):
                print(f"   upload gate: PAUSED ({gb.get('reason')}) -> no map_obj frames around this label")
        mi = None
        for tt, r in infos:
            if tt > t:
                break
            try:
                v = json.loads(r["body"])
                if v.get("valid"):
                    mi = v
            except Exception:
                pass
        win = [(tt, r) for tt, r in frames if t - w <= tt <= t + w]
        if not win:
            print(f"   (no map_obj frames within ±{args.window:g}s)")
            continue
        # map_obj has no object ids: simple nearest-neighbour tracking within the window
        # (same type+icon, moved < 0.03 map units between consecutive frames; color/blink not part of the key)
        tracks, me_at, other = [], {}, {}
        for tt, r in win:
            try:
                objs = json.loads(r["body"])
            except Exception:
                continue
            dt = (tt - t) / 1000
            me = next((o for o in objs if o.get("icon") == "Player"), None)
            if me and "x" in me:
                me_at[dt] = (me["x"], me["y"])
            used = set()
            n_en = 0
            for o in objs:
                if not is_enemy(o):
                    continue
                n_en += 1
                if o.get("type") not in ("ground_model", "aircraft"):
                    other[(o.get("type"), o.get("icon"))] = other.get((o.get("type"), o.get("icon")), 0) + 1
                    continue
                x, y = o.get("x"), o.get("y")
                best, bd = None, 0.03
                for i, tr in enumerate(tracks):
                    if i in used or tr["key"] != (o.get("type"), o.get("icon")) or tr["pts"][-1][0] == dt:
                        continue
                    lx, ly = tr["pts"][-1][3], tr["pts"][-1][4]
                    d = math.hypot(x - lx, y - ly) if None not in (x, y, lx, ly) else 1
                    if d < bd:
                        best, bd = i, d
                if best is None:
                    tracks.append({"key": (o.get("type"), o.get("icon")), "pts": []})
                    best = len(tracks) - 1
                used.add(best)
                tracks[best]["pts"].append((dt, o.get("color"), o.get("blink"), x, y, o.get("icon"), o.get("type")))
            if args.frames:
                print(f"     frame {dt:+6.2f}s seq {r.get('seq')} enemies {n_en} objs {len(objs)}")
        tl = {f"trk{i}": tr["pts"] for i, tr in enumerate(tracks)}
        nearest = min(win, key=lambda x: abs(x[0] - t))
        n_near = sum(1 for o in json.loads(nearest[1]["body"]) if is_enemy(o))
        print(f"   map_obj ±{args.window:g}s: {len(win)} frames, nearest {((nearest[0]-t)/1000):+.2f}s has {n_near} enemy marker(s); enemy unit tracks {len(tl)}"
              + (f"; other red objects: " + ", ".join(f"{k[1]}" for k in other) if other else ""))
        me = me_at.get(min(me_at, key=abs)) if me_at else None
        for key, pts in sorted(tl.items(), key=lambda kv: kv[1][0][0]):
            first, last = pts[0][0], pts[-1][0]
            variants = []
            for p in pts:
                v = f"{p[1]}/{p[2]}"
                if not variants or variants[-1][0] != v:
                    variants.append((v, p[0]))
            vs = " -> ".join(f"{v}@{d:+.1f}" for v, d in variants)
            appear = "APPEARS in window" if first > -args.window + 0.3 else ""
            gone = "GONE in window" if last < args.window - 0.3 else ""
            d = meters(mi, (pts[-1][3], pts[-1][4]), me) if me and pts[-1][3] is not None else None
            print(f"     {str(pts[0][6]):<12} {str(pts[0][5]):<14} seen {first:+.1f}..{last:+.1f}s  {vs}  {appear} {gone}" + (f"  dist {d:.0f} m" if d else ""))
            if writer:
                writer.writerow([os.path.basename(sdir), lab.get("uid"), fmt(t), lab.get("label_id"), lab.get("text"), lab.get("note") or "",
                                 lab.get("clock_synced"), ",".join(lab["tags"]), key, pts[0][6], pts[0][5], f"{first:.2f}", f"{last:.2f}", vs,
                                 f"{d:.0f}" if d else ""])
        for tt, r in huds:
            if t - w <= tt <= t + w:
                try:
                    v = json.loads(r["body"])
                except Exception:
                    continue
                for kind in ("damage", "events"):
                    for m in v.get(kind) or []:
                        msg = m.get("msg") if isinstance(m, dict) else None
                        if msg:
                            print(f"     hud {kind:<6} {(tt - t)/1000:+.1f}s  {msg[:100]}")
    return len(labels)

def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("path")
    ap.add_argument("--session", default=None)
    ap.add_argument("--window", type=float, default=3.0)
    ap.add_argument("--test", action="store_true", help="include labels tagged test")
    ap.add_argument("--undone", action="store_true", help="include undone labels")
    ap.add_argument("--frames", action="store_true", help="print every frame in the window")
    ap.add_argument("--csv", default=None, help="write one row per (label, enemy marker)")
    args = ap.parse_args()
    if glob.glob(os.path.join(args.path, "samples-*")):
        dirs = [args.path]
    else:
        dirs = sorted(os.path.dirname(p) for p in glob.glob(os.path.join(args.path, "*", "*", "labels.jsonl")))
    if args.session:
        dirs = [d for d in dirs if args.session in os.path.basename(d)]
    writer, fh = None, None
    if args.csv:
        fh = open(args.csv, "w", newline="", encoding="utf-8-sig")
        writer = csv.writer(fh)
        writer.writerow(["session", "uid", "t_server_utc8", "label_id", "text", "note", "clock_synced", "tags", "marker_id", "type", "icon", "first_dt_s", "last_dt_s", "color/blink timeline", "dist_m"])
    n = sum(analyze(d, args, writer) for d in dirs)
    if fh:
        fh.close()
    print(f"\n{n} label(s) in {len(dirs)} session(s)")

if __name__ == "__main__":
    main()
