#!/usr/bin/env python3
"""Build a v0.1.2-format spool (one gzip Batch per 250 ms batch) from real recorded records, renumbered
into a fresh synthetic session, to simulate an upload backlog. Usage: mkbacklog.py <src_session_dir> <spool_dir> <n_records>"""
import sys, glob, gzip, json, os, time
src, spool, N = sys.argv[1], sys.argv[2], int(sys.argv[3])
recs = [json.loads(l) for f in sorted(glob.glob(f"{src}/samples-*")) for l in gzip.open(f, "rt")]
recs = [r for r in recs if not r["ep"].startswith("#")]
recs.sort(key=lambda r: r["seq"])
S = time.strftime("%Y%m%d-%H%M%S", time.localtime(time.time() - 3600)) + "-backlog-test-b10c"
os.makedirs(spool, exist_ok=True)
t0 = int(time.time() * 1000) - 3600_000
ns0 = time.time_ns() - 3600 * 10**9
seq = 0; i = 0; k = 0
while seq < N:
    batch = []
    for _ in range(9):
        r = dict(recs[i % len(recs)]); i += 1
        seq += 1; r["seq"] = seq; r["session"] = S; r["wall_ms"] = t0 + seq * 28
        batch.append(r)
    k += 1
    stem = f"{ns0 + k * 250_000_000:024}-{k:06}"
    body = gzip.compress(json.dumps({"session": S, "player": "backlog-test", "version": "0.1.2", "records": batch}, ensure_ascii=False).encode(), 5)
    open(f"{spool}/{stem}.body", "wb").write(body)
    open(f"{spool}/{stem}.meta", "w").write(json.dumps({"path": "/api/ingest", "ctype": "application/json", "gzip": True}))
print(S, seq, "records", k, "entries", sum(os.path.getsize(f) for f in glob.glob(spool + "/*.body")) // 1024, "KB gz")
