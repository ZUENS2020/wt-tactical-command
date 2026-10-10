#!/usr/bin/env bash
# End-to-end test on the box with SYNTHETIC data:
# mock8111 (127.0.0.1:8111) -> wt-collector (Linux build) -> wt-receiver (100.94.249.78:18111) -> wt-analyze
# Includes a receiver outage (spool + recovery), a note via API, an offline --no-upload session + `upload` (twice, dedup).
set -u
cd /workspace/wt-collect
BIN=target/release
CFG=test/wt-collector.toml
DD=test/wt-data
DUR=${DUR:-104}
pkill -f mock/mock8111.py; sleep 0.5
nohup python3 mock/mock8111.py > test/mock.log 2>&1 &
MOCK=$!; sleep 1
echo "== live session ${DUR}s, rate cycle 2/5/10/20 Hz x 26s, receiver outage at t=40..55s"
$BIN/wt-collector --config $CFG --data-dir $DD --duration-s $DUR --rate-cycle-s 26 --note "E2E mock run (synthetic)" < /dev/null > test/collector-live.log 2>&1 &
COL=$!
sleep 30; $BIN/wt-collector --config $CFG note "note via API at t=30s" ; 
sleep 10; echo "== stopping receiver"; scripts/receiverctl.sh stop
sleep 15; echo "== starting receiver"; scripts/receiverctl.sh start >/dev/null
wait $COL; echo "collector exit=$?"
SESS=$(grep -o 'session [0-9a-z-]*' test/collector-live.log | head -1 | cut -d' ' -f2)
echo "session $SESS"
python3 - "$SESS" <<'PY'
import sys,glob,gzip,json
s=sys.argv[1]
def seqs(pat):
    out=set()
    for f in glob.glob(pat):
        op=gzip.open if f.endswith('.gz') else open
        with op(f,'rt') as fh:
            for l in fh:
                out.add(json.loads(l)['seq'])
    return out
loc=seqs(f'test/wt-data/{s}/samples-*'); rem=seqs(f'data/*/{s}/samples-*')
print(f"local records {len(loc)}  receiver records {len(rem)}  missing-on-receiver {len(loc-rem)}  extra {len(rem-loc)}")
print("LOSSLESS" if loc==rem and loc else "MISMATCH")
PY
ls test/wt-data/spool | wc -l | xargs echo "spool entries left:"
grep -E "BACKLOG|rate-cycle" test/collector-live.log | head -n 4
echo "== offline session (--no-upload) then upload twice"
$BIN/wt-collector --config $CFG --data-dir $DD --duration-s 8 --no-upload --player mock-offline < /dev/null > test/collector-offline.log 2>&1
OFF=$(grep -o 'session [0-9a-z-]*' test/collector-offline.log | head -1 | cut -d' ' -f2)
$BIN/wt-collector --config $CFG upload $DD/$OFF | tail -n 3
$BIN/wt-collector --config $CFG upload $DD/$OFF | tail -n 1
curl -s http://100.94.249.78:18111/api/status | python3 -c "
import sys,json; d=json.load(sys.stdin)
for s in d['live_sessions']: print(s['session'], 'records', s['records'], 'dups', s['dups'])"
kill $MOCK
echo "== analyze"
$BIN/wt-analyze "$SESS"
echo "SESSION=$SESS OFFLINE=$OFF"
