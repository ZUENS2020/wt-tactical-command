"""Cut small real-data fixtures from the HK receiver copy. respawn_base_* objects are removed from map_obj bodies
(up to 246 per frame, irrelevant here) to keep the files small; everything else is byte-for-byte."""
import gzip, json, glob, sys, datetime as dt
SRC = "/workspace/hkdata/2026-10-10/20261010-193009-shengyuan-1709"
TZ = dt.timezone(dt.timedelta(hours=8))
def ms(hms): return int(dt.datetime(2026, 10, 10, *map(int, hms.split(":")), tzinfo=TZ).timestamp() * 1000)
WINDOWS = {
    "b1_location_calls_1709.jsonl.gz": ("20:05:58", "20:07:10"),
    "b2_squad_spawn_1709.jsonl.gz": ("20:09:50", "20:10:12"),
    "b3_spectate_1709.jsonl.gz": ("20:19:00", "20:19:20"),
    "b4_pings_squad_air_1709.jsonl.gz": ("20:27:00", "20:28:25"),
}
EPS = {"/map_obj.json", "/map_info.json", "/gamechat"}
recs = []
for f in sorted(glob.glob(SRC + "/samples-*")):
    for l in gzip.open(f, "rt"):
        r = json.loads(l)
        if r["ep"] in EPS:
            recs.append(r)
last_info = {}
for name, (a, b) in WINDOWS.items():
    t0, t1 = ms(a), ms(b)
    out = []
    info = max((r for r in recs if r["ep"] == "/map_info.json" and r["wall_ms"] < t0 and r.get("body")), key=lambda r: r["wall_ms"])
    out.append(info)
    for r in recs:
        if t0 <= r["wall_ms"] <= t1:
            r = dict(r)
            if r["ep"] == "/map_obj.json" and r.get("body"):
                objs = json.loads(r["body"])
                r["body"] = json.dumps([o for o in objs if not str(o.get("type", "")).startswith("respawn_base")], ensure_ascii=False)
            out.append(r)
    with gzip.open("allies/tests/fixtures/" + name, "wt") as fh:
        for r in out:
            fh.write(json.dumps(r, ensure_ascii=False) + "\n")
    print(name, len(out))
