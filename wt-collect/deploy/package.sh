#!/usr/bin/env bash
# Render README/toml for a server URL and build dist/wt-collector-win64.zip. Usage: package.sh <server_url>
set -euo pipefail
cd /workspace/wt-collect
URL=${1:?server url}
HOST=$(python3 -c "import sys,urllib.parse;print(urllib.parse.urlparse(sys.argv[1]).netloc)" "$URL")
cp target/x86_64-pc-windows-gnu/release/wt-collector.exe dist/pkg/wt-collector.exe
sed "s#{SERVER}#$URL#g; s#{HOST}#$HOST#g" dist/pkg/README.tmpl.txt > dist/pkg/README.txt
sed "s#{SERVER}#$URL#g" dist/pkg/wt-collector.tmpl.toml > dist/pkg/wt-collector.toml
python3 - <<'PY'
import zipfile
with zipfile.ZipFile('dist/wt-collector-win64.zip.tmp','w',zipfile.ZIP_DEFLATED,compresslevel=9) as z:
    for f in ['wt-collector.exe','wt-collector.toml','README.txt','diag.bat','cli-run.bat']:
        z.write('dist/pkg/'+f,'wt-collector/'+f)
PY
mv dist/wt-collector-win64.zip.tmp dist/wt-collector-win64.zip
echo "packaged $(stat -c %s dist/wt-collector-win64.zip) bytes, server=$(grep '^server' dist/pkg/wt-collector.toml)"
