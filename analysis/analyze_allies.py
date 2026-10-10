"""Squadmates + map pings over all recorded sessions. Usage: analyze_allies.py [data_root] [out.json]"""
import glob, gzip, json, os, sys, statistics, collections, datetime as dt
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from allies import session
from allies.grid import cell_of

ROOT = sys.argv[1] if len(sys.argv) > 1 else "/workspace/hkdata"
OUT = sys.argv[2] if len(sys.argv) > 2 else os.path.join(os.path.dirname(os.path.abspath(__file__)), "allies_results.json")
TZ = dt.timezone(dt.timedelta(hours=8))
T = lambda ms: dt.datetime.fromtimestamp(ms / 1000, TZ).strftime("%H:%M:%S.%f")[:-3]


def load(sdir):
    recs = []
    for f in sorted(glob.glob(sdir + "/samples-*.jsonl*")):
        op = gzip.open if f.endswith(".gz") else open
        try:
            with op(f, "rt") as fh:
                for l in fh:
                    try: recs.append(json.loads(l))
                    except ValueError: pass
        except EOFError:
            pass
    return recs


def main():
    res = {}
    for sdir in sorted(glob.glob(ROOT + "/*/*/")):
        sid = os.path.basename(sdir.rstrip("/"))
        out = session.run(load(sdir.rstrip("/")))
        tr, pings = out["tracker"], out["pings"]
        if not tr.tracks and not out["msgs"]:
            continue
        sq = [t for t in tr.tracks if t.role == "squad"]
        by_cat = collections.Counter(p.category for p in pings)
        dur = collections.defaultdict(list)
        for p in pings:
            if p.signal_ms is not None:
                dur[(p.category, p.kind)].append(p.signal_ms)
        loc_ok = [p.grid == p.sender_cell for p in pings if p.category == "location" and p.sender_cell and p.sender_icon not in ("Fighter", "Bomber", "Assault")]
        ping_same = [p.grid == p.sender_cell for p in pings if p.category == "ping" and p.sender_cell and p.grid]
        res[sid] = {
            "battles": out["battles"],
            "squad_tracks": [dict(t.to_dict(), t0=T(t.first_seen), t1=T(t.last_seen), dur_s=round((t.last_seen - t.first_seen) / 1000, 1)) for t in sq],
            "squad_names_from_chat": out["squad_names"],
            "pings": [dict(p.to_dict(), t=T(p.t_ms)) for p in pings],
            "counts": dict(by_cat),
            "signal_ms_by_kind": {f"{k[0]}/{k[1]}": {"n": len(v), "median": statistics.median(v), "min": min(v), "max": max(v)} for k, v in dur.items()},
            "location_grid_equals_sender_cell": [sum(loc_ok), len(loc_ok)],
            "ping_grid_equals_sender_cell": [sum(ping_same), len(ping_same)],
            "markers": [m.__dict__ | {"key": list(m.key)} for m in tr.marker_log],
            "msgs_total": len(out["msgs"]), "msgs_backlog": sum(m.backlog for m in out["msgs"]),
        }
        r = res[sid]
        print(f"== {sid}: squad tracks {len(sq)}, events {r['counts']}, location grid==sender {r['location_grid_equals_sender_cell']}, ping grid==sender {r['ping_grid_equals_sender_cell']}, markers {len(r['markers'])}, chat {r['msgs_total']} (backlog {r['msgs_backlog']})")
        for t in r["squad_tracks"]:
            print(f"   squad #{t['id']} {t['type']}/{t['icon']} {t['t0']}-{t['t1']} ({t['dur_s']} s) signals {t['signals']}")
        for k, v in r["signal_ms_by_kind"].items():
            print(f"   blink {k}: {v}")
        for p in pings:
            print(f"   {T(p.t_ms)} {p.category:8s} {p.kind:18s} {str(p.sender)[:16]:16s} role={p.sender_role:7s} grid={p.grid} sender_cell={p.sender_cell} blink={p.signal_ms} {p.source}")
    json.dump(res, open(OUT, "w"), ensure_ascii=False, indent=1, default=str)
    print("->", OUT)


if __name__ == "__main__":
    main()
