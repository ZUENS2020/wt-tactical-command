#!/usr/bin/env bash
# package.sh SERVER_URL [EXE_PATH]; credentials stay out of command arguments and templates.
set -euo pipefail
TASK_PACKAGE_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
exec python3 "$TASK_PACKAGE_DIR/package.py" "$@"
