#!/usr/bin/env bash
# 0.2.0 local e2e: mock (long hangar) -> wt-collector (linux, CLI run) -> local wt-receiver (127.0.0.1:18199)
# Checks: hangar pause (#state paused/active, heartbeats, no repeated hangar records), resume, labels via CLI.
set -uo pipefail
cd /workspace/wt-collect
T=test/gate; rm -rf $T/data; mkdir -p $T/data
TOK=localtesttoken-0123456789abcdef
pkill -f "mock8111.py --port 8111" ; pkill -f "WT_BIND=127.0.0.1:18199"; sleep 0.3
python3 mock/mock8111.py --port 8111 --hangar-s ${HANGAR_S:-70} > $T/mock.log 2>&1 &
MOCK=$!
WT_TOKEN=$TOK WT_BIND=127.0.0.1:18199 WT_DATA=$T/data target/release/wt-receiver > $T/receiver.log 2>&1 &
RCV=$!
sleep 0.5
printf 'server = "http://127.0.0.1:18199"\ntoken = "%s"\nplayer = "gatetest"\ngame_port = 8111\n' $TOK > $T/cfg.toml
timeout 200 target/release/wt-collector --config $T/cfg.toml --duration-s ${DUR:-110} > $T/collector.log 2>&1 &
COL=$!
sleep 8
SID=$(ls $T/data/*/ | grep gatetest | head -1)
echo "session $SID"
# a label while in hangar/paused (CLI, attached to the running session; seq space separate)
target/release/wt-collector --config $T/cfg.toml label i_spotted --session "$SID" --note "hangar label test" >> $T/label.log 2>&1
wait $COL
target/release/wt-collector --config $T/cfg.toml label kill --session "$SID" --tag test >> $T/label.log 2>&1
curl -s -H "Authorization: Bearer $TOK" "http://127.0.0.1:18199/api/labels?session=$SID&test=1&meta=1" > $T/labels.json
curl -s "http://127.0.0.1:18199/api/status" > $T/status.json
kill $RCV $MOCK
python3 - "$T/data" "$SID" <<'PY'
import gzip, json, sys, glob, collections
root, sid = sys.argv[1], sys.argv[2]
recs = []
for f in sorted(glob.glob(f"{root}/*/{sid}/samples-*.jsonl.gz")):
    with gzip.open(f, "rt") as fh:
        recs += [json.loads(l) for l in fh if l.strip()]
recs.sort(key=lambda r: r["seq"])
eps = collections.Counter(r["ep"] for r in recs)
print("records", len(recs), dict(eps))
t0 = recs[0]["wall_ms"]
for r in recs:
    if r["ep"] in ("#state", "#heartbeat", "#label"):
        b = json.loads(r["body"])
        print(f'{(r["wall_ms"]-t0)/1000:7.1f}s {r["ep"]:<10} ' + (f'{b.get("event")} paused={b.get("paused")} reason={b.get("reason")} suppressed={b.get("suppressed_total")}' if r["ep"] != "#label" else f'{b["label_id"]} {b["text"]} note={b.get("note")} source={b["source"]} late={b["late"]} src={b["t_server_source"]}'))
# map_obj records during paused windows must be absent
paused = []
cur = None
for r in recs:
    if r["ep"] == "#state":
        b = json.loads(r["body"])
        if b["paused"] and cur is None: cur = r["wall_ms"]
        if not b["paused"] and cur is not None: paused.append((cur, r["wall_ms"])); cur = None
if cur is not None: paused.append((cur, recs[-1]["wall_ms"] + 1))
inside = [r for r in recs if not r["ep"].startswith("#") and any(a < r["wall_ms"] < b - 2500 for a, b in paused)]
print("paused windows (s):", [round((b-a)/1000,1) for a,b in paused], "telemetry records inside paused windows:", len(inside))
mo = [r for r in recs if r["ep"] == "/map_obj.json"]
print("map_obj records", len(mo), "non-empty", sum(1 for r in mo if r.get("body","[]").strip() not in ("[]", "")))
PY
python3 -c "import json;d=json.load(open('$T/labels.json'));print('api/labels count',d['count']);[print(' ',l['label_id'],l['text'],l.get('note'),'tags',l['tags'],'undone',l['undone']) for l in d['labels']]"
grep -E "暂停|恢复|PAUSED" $T/collector.log | head -8
