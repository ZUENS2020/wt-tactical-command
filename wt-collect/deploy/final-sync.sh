#!/usr/bin/env bash
# Top-up merge of box sessions into the HK server via the receiver API (dedupe by session+seq, safe to repeat).
# Run after the PC has switched to the new server URL. Usage: final-sync.sh [server_url]
set -euo pipefail
cd /workspace/wt-collect
URL=${1:-http://43.129.251.18}
umask 077
CFG=$(mktemp /workspace/wt-collect/deploy/.sync-XXXX.toml)
trap 'rm -f "$CFG"' EXIT
TOKEN=$(grep '^WT_TOKEN=' receiver.env | cut -d= -f2-)
printf 'server = "%s"\ntoken = "%s"\nplayer = "sync"\n' "$URL" "$TOKEN" > "$CFG"
for d in data/*/; do
  target/release/wt-collector --config "$CFG" upload "$d" 2>&1 | grep -v "^  image" | tr '\r' '\n' | grep -v '^  [0-9]*/' || true
done
echo "== box"; python3 deploy/count.py data
echo "== server"; ssh -i ~/.ssh/wt_hk_ed25519 -o BatchMode=yes ubuntu@43.129.251.18 'sudo python3 - /var/lib/wt-receiver/data' < deploy/count.py
