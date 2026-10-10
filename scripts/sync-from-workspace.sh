#!/usr/bin/env bash
# Re-sync the live working dirs into this monorepo with the same excludes as the initial import.
# Usage: scripts/sync-from-workspace.sh [--dry-run]
# Then review `git status` / run gitleaks, and commit.
set -euo pipefail
REPO=$(cd "$(dirname "$0")/.." && pwd)
mkdir -p "$REPO/docs/plan" "$REPO/docs/research"
WS=${WS:-/workspace}
DRY=""; [[ "${1:-}" == "--dry-run" ]] && DRY="--dry-run"
RS=(rsync -a --delete --delete-excluded $DRY --prune-empty-dirs)

# common excludes: build outputs, deps, caches, logs, secrets, VCS
COMMON=(
  --exclude='.git/' --exclude='target/' --exclude='node_modules/' --exclude='dist/' --exclude='build-output/'
  --exclude='*.zip' --exclude='*.deb' --exclude='*.exe' --exclude='*.dll' --exclude='*.pdb'
  --exclude='logs/' --exclude='*.log' --exclude='*.pid'
  --exclude='__pycache__/' --exclude='*.pyc' --exclude='.venv/' --exclude='venv/' --exclude='*.pkl'
  --exclude='data/' --exclude='spool/' --exclude='*.jsonl'
  --exclude='*.env' --exclude='.env*' --exclude='receiver.env' --exclude='.secrets/' --exclude='*token*' --exclude='*secret*'
  --exclude='id_rsa*' --exclude='id_ed25519*' --exclude='*_ed25519*' --exclude='*.pem' --exclude='*.key' --exclude='.sync-*.toml'
  --exclude='*.bak' --exclude='*.tmp'
)

# 1) Rust workspace (keep only dist/pkg templates; drop recorded test sessions, binaries, local configs with tokens)
"${RS[@]}" --max-size=2m \
  --include='/dist/' --include='/dist/pkg/' --include='/dist/pkg/*.tmpl.*' --include='/dist/pkg/diag.bat' --exclude='/dist/**' \
  --exclude='/bin/' --exclude='/test/win*/' --exclude='/test/wt-data/' --exclude='/test/e2e-sessions/' \
  --exclude='/test/clk-rx/' --exclude='/test/rtt-rx/' --exclude='/test/gate/data/' --exclude='/test/naval-it/' \
  --exclude='/test/wt-collector.toml' --exclude='/test/*.before' --exclude='/test/*.after' --exclude='/test/delay.ms' \
  --exclude='/test/gate/status.json' --exclude='/test/gate/labels.json' \
  --exclude='/backup-*.tar.gz' --exclude='*.tar.gz' "${COMMON[@]}" \
  "$WS/wt-collect/" "$REPO/wt-collect/"

# 2) web prototype
"${RS[@]}" --max-size=2m --include='/src/data/' "${COMMON[@]}" "$WS/wt-proto/" "$REPO/web-proto/"

# 3) analysis (python modules, reports, small fixtures; no venv / big map images / pickles)
"${RS[@]}" --max-size=2m "${COMMON[@]}" "$WS/wt-analysis/" "$REPO/analysis/"

# 4) plan docs (md + pdf + build scripts)
"${RS[@]}" --max-size=10m "${COMMON[@]}" "$WS/wt-plan/" "$REPO/docs/plan/"

# 5) research: only own notes / markdown / screenshots at top level (third-party clones -> REFERENCES.md)
"${RS[@]}" --max-size=2m --filter='P /REFERENCES.md' --include='/*.md' --include='/*.png' --include='/*.txt' --exclude='*' \
  "$WS/wt-research/" "$REPO/docs/research/"

# sanitize: replace any live receiver token that ended up in templates/configs with a placeholder
TOKEN_FILE="$WS/wt-collect/receiver.env"
if [[ -z "$DRY" && -f "$TOKEN_FILE" ]]; then
  TOK=$(grep '^WT_TOKEN=' "$TOKEN_FILE" | cut -d= -f2- || true)
  if [[ -n "$TOK" ]]; then
    grep -rlF --exclude-dir=.git -- "$TOK" "$REPO" 2>/dev/null | while read -r f; do
      sed -i "s|$TOK|CHANGE_ME_WT_TOKEN|g" "$f"; echo "sanitized token in ${f#$REPO/}"
    done
  fi
fi
echo "sync done -> $REPO"
