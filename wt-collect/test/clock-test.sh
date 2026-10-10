#!/usr/bin/env bash
# Clock-skew / outage / late-delivery test. Usage: clock-test.sh <label> <faketime offset e.g. +45s> <linux|wine>
set -u
cd /workspace/wt-collect
LABEL=$1; SKEW=$2; MODE=$3
W=test/clk-$LABEL; rm -rf $W; mkdir -p $W
echo 100 > test/delay.ms
pgrep -f "WT_BIND=127.0.0.1:18199" >/dev/null
if ! curl -s -m 2 http://127.0.0.1:18199/healthz >/dev/null; then
  (WT_TOKEN=testtoken0123456789abcd WT_BIND=127.0.0.1:18199 WT_DATA=$PWD/test/clk-rx nohup target/release/wt-receiver >> test/clk-rx.log 2>&1 &)
fi
pkill -f "delayproxy.py --listen 18299"; pkill -f "mock8111.py --port 9222"; sleep 0.3
(nohup python3 mock/delayproxy.py --listen 18299 --target 127.0.0.1:18199 --delay-ms 100 --delay-file test/delay.ms > test/proxy.log 2>&1 &)
(nohup python3 mock/mock8111.py --port 9222 > test/mock9222.log 2>&1 &)
sleep 1
printf 'server = "http://127.0.0.1:18299"\ntoken = "testtoken0123456789abcd"\nplayer = "%s"\ngame_host = "127.0.0.1"\ngame_port = 9222\n' "$LABEL" > $W/wt-collector.toml
if [ $MODE = wine ]; then cp target/x86_64-pc-windows-gnu/release/wt-collector.exe $W/; fi
(cd $W && find . -printf '%p %s %T@\n' | sort) > test/clk-$LABEL.before
T0=$(date +%s)
if [ $MODE = wine ]; then
  (cd $W && WINEDEBUG=-all faketime -f "$SKEW" timeout 120 /usr/lib/wine/wine64 wt-collector.exe --duration-s 75 > ../clk-$LABEL.log 2>&1) &
else
  (cd $W && faketime -f "$SKEW" timeout 120 ../../target/release/wt-collector --config wt-collector.toml --duration-s 75 > ../clk-$LABEL.log 2>&1) &
fi
PID=$!
at() { while [ $(( $(date +%s) - T0 )) -lt $1 ]; do sleep 0.2; done; }
at 20; echo "t=20 outage start"; pkill -f "delayproxy.py --listen 18299"
at 30; echo "t=30 outage end"; (nohup python3 mock/delayproxy.py --listen 18299 --target 127.0.0.1:18199 --delay-ms 100 --delay-file test/delay.ms >> test/proxy.log 2>&1 &)
at 42; echo "t=42 link delay 3600 ms one-way"; echo 3600 > test/delay.ms
at 52; echo "t=52 link delay back to 100 ms"; echo 100 > test/delay.ms
wait $PID
echo "collector exit $?"
(cd $W && find . -printf '%p %s %T@\n' | sort) > test/clk-$LABEL.after
if diff -q test/clk-$LABEL.before test/clk-$LABEL.after >/dev/null; then echo "DISK: collector folder unchanged (no files written)"; else echo "DISK: CHANGED:"; diff test/clk-$LABEL.before test/clk-$LABEL.after; fi
pkill -f "mock8111.py --port 9222"
