"""Build a portable Windows release ZIP. Set WT_PACKAGE_TOKEN_FILE for a configured internal release."""
import hashlib, os, pathlib, struct, sys, urllib.parse, zipfile
root = pathlib.Path(__file__).resolve().parents[1]
url = (sys.argv[1] if len(sys.argv) > 1 else '').rstrip('/')
u = urllib.parse.urlsplit(url)
if u.scheme not in ('http', 'https') or not u.hostname or u.username or u.password:
    raise SystemExit('Usage: package.sh SERVER_URL [EXE_PATH]')
candidates = [root/'target/release/wt-collector.exe', root/'target/x86_64-pc-windows-msvc/release/wt-collector.exe', root/'target/x86_64-pc-windows-gnu/release/wt-collector.exe']
exe = pathlib.Path(sys.argv[2]) if len(sys.argv) > 2 else next((p for p in candidates if p.is_file()), None)
if exe is None or not exe.is_file(): raise SystemExit('Windows exe not found; pass EXE_PATH')
data = exe.read_bytes()
pe = struct.unpack_from('<I', data, 0x3c)[0]
if data[:2] != b'MZ' or data[pe:pe+4] != b'PE\0\0' or struct.unpack_from('<H', data, pe+4)[0] != 0x8664:
    raise SystemExit('Expected a Windows x64 PE executable')
config = (root/'dist/pkg/wt-collector.tmpl.toml').read_text().replace('{SERVER}', url)
if os.environ.get('WT_PACKAGE_TOKEN_FILE'):
    import json
    token = pathlib.Path(os.environ['WT_PACKAGE_TOKEN_FILE']).read_text().strip()
    if not token or any(c in token for c in '\r\n'): raise SystemExit('Invalid upload token file')
    config = config.replace('"CHANGE_ME_WT_TOKEN"', json.dumps(token))
readme = (root/'dist/pkg/README.tmpl.txt').read_text().replace('{SERVER}', url).replace('{HOST}', u.netloc)
files = {'wt-collector.exe': data, 'wt-collector.toml': config.encode('utf-8-sig'), 'README.txt': readme.replace('\n','\r\n').encode('utf-8-sig'),
         'diag.bat': (root/'dist/pkg/diag.bat').read_bytes(), 'cli-run.bat': b'@echo off\r\ncd /d "%~dp0"\r\nwt-collector.exe run\r\npause\r\n'}
out = root/'dist/wt-collector-win64.zip'
tmp = out.with_suffix('.zip.tmp')
with zipfile.ZipFile(tmp,'w',zipfile.ZIP_DEFLATED,compresslevel=9) as z:
    for name, body in files.items(): z.writestr('wt-collector/'+name,body)
tmp.replace(out)
print(f'packaged {out.name}, {out.stat().st_size} bytes, SHA256 {hashlib.sha256(out.read_bytes()).hexdigest()}')
