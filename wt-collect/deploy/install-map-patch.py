"""Promote a verified receiver/web patch without changing config or client downloads.

Run as root: python3 install-map-patch.py STAGING_DIR RELEASE_TAG
STAGING_DIR contains receiver, web.tar.gz and expected.json (artifact SHA256s).
"""
import hashlib
import json
import os
import pathlib
import shutil
import signal
import subprocess
import sys
import tarfile
import time
import urllib.request

stage, tag = pathlib.Path(sys.argv[1]), sys.argv[2]
if not tag or any(c not in 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_' for c in tag):
    raise SystemExit('Invalid release tag')
expected = json.loads((stage/'expected.json').read_text())
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
for name in ['receiver', 'web.tar.gz']:
    if sha(stage/name) != expected[name]: raise SystemExit('Artifact checksum mismatch: '+name)
if (stage/'receiver').read_bytes()[:4] != b'\x7fELF': raise SystemExit('Not an ELF receiver')
root, live = pathlib.Path('/opt/wt-receiver'), pathlib.Path('/var/www/wt-live')
if not live.is_symlink(): raise SystemExit('Expected the existing managed web symlink')
release, backup = root/'releases'/tag, root/'backups'/tag
if release.exists() or backup.exists(): raise SystemExit('Release tag already exists')
old_web = os.readlink(live)
release.mkdir(mode=0o755)
shutil.copy2(stage/'receiver', release/'wt-receiver'); (release/'wt-receiver').chmod(0o755)
web = release/'web'; web.mkdir(mode=0o755)
with tarfile.open(stage/'web.tar.gz') as archive:
    for member in archive.getmembers():
        p = pathlib.PurePosixPath(member.name)
        if p.is_absolute() or '..' in p.parts or member.issym() or member.islnk(): raise SystemExit('Unsafe archive member')
    archive.extractall(web)
for p in web.rglob('*'): p.chmod(0o755 if p.is_dir() else 0o644)
(web/'downloads').symlink_to('/var/www/wt-downloads', target_is_directory=True)
if sha(web/'index.html') != expected['index.html']: raise SystemExit('Extracted web checksum mismatch')
(stage/'receiver').chmod(0o755)

def healthy(port):
    for _ in range(40):
        try:
            if urllib.request.urlopen(f'http://127.0.0.1:{port}/healthz', timeout=1).read().strip() == b'ok': return True
        except Exception: pass
        time.sleep(.2)
    return False

# Exercise the Linux release itself in a separate data directory before promotion.
env = dict(os.environ, WT_DATA=str(stage/'smoke-data'), WT_LIVE_DIR=str(stage/'smoke-live'),
    WT_BIND='127.0.0.1:18113', WT_TOKEN='synthetic-patch-upload', WT_VIEW_TOKEN='synthetic-patch-view', WT_LIVE_ROOM='patch-test')
with (stage/'linux-smoke.log').open('wb') as log:
    smoke = subprocess.Popen([str(stage/'receiver')], env=env, stdout=log, stderr=log)
    try:
        if not healthy(18113) or smoke.poll() is not None: raise SystemExit('Linux receiver smoke test failed')
    finally:
        if smoke.poll() is None: smoke.send_signal(signal.SIGTERM)
        try: smoke.wait(timeout=10)
        except subprocess.TimeoutExpired: smoke.kill(); smoke.wait()

backup.mkdir(mode=0o700)
shutil.copy2(root/'bin/wt-receiver', backup/'wt-receiver')
(backup/'previous-web.txt').write_text(old_web+'\n')
previous_sha = sha(backup/'wt-receiver')
stopped = False
try:
    subprocess.run(['systemctl', 'stop', 'wt-receiver'], check=True); stopped = True
    # Stop waits for the room actor's final save; preserve current drawings before restart.
    for p in pathlib.Path('/var/lib/wt-receiver/live').glob('room-*.json'): shutil.copy2(p, backup/p.name)
    tmp = root/'bin/wt-receiver.patch'; shutil.copy2(stage/'receiver', tmp); tmp.chmod(0o755); tmp.replace(root/'bin/wt-receiver')
    subprocess.run(['systemctl', 'start', 'wt-receiver'], check=True)
    if not healthy(18111): raise RuntimeError('Receiver health check failed after promotion')
    link = live.with_name('wt-live.patch'); link.unlink(missing_ok=True); link.symlink_to(web, target_is_directory=True); link.replace(live)
    manifest = {**expected, 'tag': tag, 'previous_web': old_web, 'previous_receiver_sha256': previous_sha, 'linux_smoke_passed': True}
    (release/'manifest.json').write_text(json.dumps(manifest, indent=2)+'\n')
    print(json.dumps({'tag': tag, 'receiver_sha256': expected['receiver'], 'index_sha256': expected['index.html'], 'healthy': True, 'previous_web': old_web}))
except BaseException:
    if stopped:
        subprocess.run(['systemctl', 'stop', 'wt-receiver'], check=False)
        tmp = root/'bin/wt-receiver.rollback'; shutil.copy2(backup/'wt-receiver', tmp); tmp.replace(root/'bin/wt-receiver')
        link = live.with_name('wt-live.rollback'); link.unlink(missing_ok=True); link.symlink_to(old_web, target_is_directory=True); link.replace(live)
        subprocess.run(['systemctl', 'start', 'wt-receiver'], check=True)
    raise
