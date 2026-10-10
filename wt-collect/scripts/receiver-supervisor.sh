#!/usr/bin/env bash
# Keeps wt-receiver running (auto-restart on crash). Started by scripts/receiverctl.sh start.
cd /workspace/wt-collect
set -a; . ./receiver.env; set +a
mkdir -p logs
while true; do
  echo "[$(date '+%F %T')] supervisor: starting wt-receiver" >> logs/receiver.log
  ./bin/wt-receiver >> logs/receiver.log 2>&1
  rc=$?
  echo "[$(date '+%F %T')] supervisor: wt-receiver exited rc=$rc, restarting in 2s" >> logs/receiver.log
  [ -f logs/stop ] && { rm -f logs/stop; echo "[$(date '+%F %T')] supervisor: stop requested" >> logs/receiver.log; exit 0; }
  sleep 2
done
