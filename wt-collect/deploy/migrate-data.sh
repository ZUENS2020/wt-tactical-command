#!/usr/bin/env bash
# Full copy of the box receiver data to wt-tactical-hk (both receivers briefly stopped for a consistent snapshot),
# then count records/images per session on both sides.
set -euo pipefail
cd /workspace/wt-collect
K=~/.ssh/wt_hk_ed25519; H=ubuntu@43.129.251.18
SSH="ssh -i $K -o BatchMode=yes $H"
$SSH 'sudo systemctl stop wt-receiver'
scripts/receiverctl.sh stop
trap "scripts/receiverctl.sh start >/dev/null; $SSH sudo systemctl start wt-receiver" ERR
rsync -a -e "ssh -i $K -o BatchMode=yes" --rsync-path="sudo rsync" data/ $H:/var/lib/wt-receiver/data/
scripts/receiverctl.sh start >/dev/null
$SSH 'sudo chown -R wt-receiver:wt-receiver /var/lib/wt-receiver/data && sudo systemctl start wt-receiver'
echo "== box"; python3 deploy/count.py data
echo "== server"; $SSH 'sudo python3 - /var/lib/wt-receiver/data' < deploy/count.py
