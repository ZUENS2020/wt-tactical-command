"""Install an already verified v2 release on the Linux host, with backup and rollback.

Run as root: python3 install-live.py STAGING_DIRECTORY RELEASE_TAG.
Staging contains receiver, web/, and the public token-placeholder collector.zip.
No credentials are printed or embedded into the public download.
"""
import ctypes
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time
import urllib.request
import zipfile

stage, tag = Path(sys.argv[1]).resolve(), sys.argv[2]
assert os.geteuid() == 0, 'run as root'
assert re.fullmatch(r'[A-Za-z0-9_.-]{1,64}', tag), 'invalid release tag'
with zipfile.ZipFile(stage / 'collector.zip') as archive:
    config = archive.read('wt-collector/wt-collector.toml').decode('utf-8-sig')
    assert re.search(r'^token\s*=\s*"CHANGE_ME_WT_TOKEN"\s*$', config, re.M), 'public ZIP must not contain the upload credential'

backup = Path('/opt/wt-receiver/backups') / tag
release = Path('/opt/wt-receiver/releases') / tag
binary = Path('/opt/wt-receiver/bin/wt-receiver')
env_file = Path('/opt/wt-receiver/receiver.env')
live = Path('/var/lib/wt-receiver/live')
web_root = Path('/var/www/wt-live')
downloads = Path('/var/www/wt-downloads')
public_zip = downloads / 'wt-collector-win64.zip'
public_sha = downloads / 'wt-collector-win64.zip.sha256'
server_changed = web_changed = zip_changed = download_link_added = False
old_web = None

def run(*args):
    return subprocess.run(args, check=True)

def exchange(a, b):
    libc = ctypes.CDLL(None, use_errno=True)
    rename = libc.renameat2
    rename.argtypes = [ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_uint]
    if rename(-100, os.fsencode(a), -100, os.fsencode(b), 2) != 0:
        raise OSError(ctypes.get_errno(), 'atomic web directory exchange failed')

def health():
    for _ in range(60):
        try:
            if urllib.request.urlopen('http://127.0.0.1:18111/healthz', timeout=2).read() == b'ok':
                return
        except OSError:
            pass
        time.sleep(.5)
    raise RuntimeError('receiver health check timed out')

backup.mkdir(parents=True, mode=0o700, exist_ok=False)
os.chmod(backup, 0o700)
release.mkdir(parents=True, mode=0o755, exist_ok=False)
shutil.copy2(binary, backup / 'receiver')
shutil.copy2(env_file, backup / 'receiver.env')
shutil.copy2('/etc/caddy/Caddyfile', backup / 'Caddyfile')
shutil.copytree(web_root, backup / 'web', symlinks=True)
for source in [public_zip, public_sha]:
    if source.exists():
        shutil.copy2(source, backup / source.name)
shutil.copytree(stage / 'web', release / 'web')
for p in (release / 'web').rglob('*'):
    os.chmod(p, 0o755 if p.is_dir() else 0o644)
shutil.copy2(stage / 'receiver', release / 'receiver')
os.chmod(release / 'receiver', 0o755)

try:
    print('BACKUP_READY', tag, flush=True)
    run('systemctl', 'stop', 'wt-receiver')
    server_changed = True
    shutil.copytree(live, backup / 'live', symlinks=True)
    text = env_file.read_text()
    lines = [line for line in text.splitlines() if line.split('=', 1)[0].strip() not in ('WT_MAX_SKEW_MS', 'WT_ACCEPT_LEGACY')]
    new_env = env_file.with_suffix('.env.next')
    new_env.write_text('\n'.join(lines) + '\nWT_MAX_SKEW_MS=2000\nWT_ACCEPT_LEGACY=off\n')
    os.chmod(new_env, 0o600)
    os.replace(new_env, env_file)
    next_binary = binary.with_suffix('.next')
    shutil.copy2(release / 'receiver', next_binary)
    os.chmod(next_binary, 0o755)
    os.replace(next_binary, binary)
    run('systemctl', 'start', 'wt-receiver')
    health()
    print('RECEIVER_HEALTHY', flush=True)

    downloads.mkdir(mode=0o755, exist_ok=True)
    zip_changed = True
    next_zip = public_zip.with_suffix('.zip.next')
    shutil.copy2(stage / 'collector.zip', next_zip)
    os.chmod(next_zip, 0o644)
    os.replace(next_zip, public_zip)
    sha = hashlib.sha256(public_zip.read_bytes()).hexdigest()
    next_sha = public_sha.with_suffix('.sha256.next')
    next_sha.write_text(sha + '  wt-collector-win64.zip\n')
    os.chmod(next_sha, 0o644)
    os.replace(next_sha, public_sha)
    if not (web_root / 'downloads').exists():
        (web_root / 'downloads').symlink_to(downloads, target_is_directory=True)
        download_link_added = True
    print('COLLECTOR_PUBLISHED', sha, flush=True)

    (release / 'web' / 'downloads').symlink_to(downloads, target_is_directory=True)
    next_root = web_root.with_name('wt-live.next-' + tag)
    next_root.symlink_to(release / 'web', target_is_directory=True)
    exchange(web_root, next_root)
    web_changed = True
    old_web = next_root
    previous = web_root.with_name('wt-live.previous-' + tag)
    os.replace(next_root, previous)
    old_web = previous
    health()
    manifest = {'release': tag, 'receiver_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
                'collector_sha256': sha, 'web_index_sha256': hashlib.sha256((web_root / 'index.html').read_bytes()).hexdigest(),
                'backup': str(backup), 'previous_web': str(previous), 'guard_ms': 2000, 'legacy': 'off'}
    (release / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    print(json.dumps(manifest), flush=True)
    print('DEPLOYMENT_COMPLETE', flush=True)
except BaseException:
    print('DEPLOYMENT_FAILED_ROLLING_BACK', file=sys.stderr, flush=True)
    if web_changed and old_web:
        rollback = web_root.with_name('wt-live.rollback-' + tag)
        rollback.symlink_to(old_web, target_is_directory=True)
        os.replace(rollback, web_root)
    if download_link_added and (web_root / 'downloads').is_symlink():
        (web_root / 'downloads').unlink()
    if zip_changed:
        for dest in [public_zip, public_sha]:
            if (backup / dest.name).exists():
                shutil.copy2(backup / dest.name, dest)
            else:
                dest.unlink(missing_ok=True)
    if server_changed:
        run('systemctl', 'stop', 'wt-receiver')
        shutil.copy2(backup / 'receiver', binary)
        shutil.copy2(backup / 'receiver.env', env_file)
        for p in (backup / 'live').rglob('*'):
            if p.is_file():
                dest = live / p.relative_to(backup / 'live')
                shutil.copy2(p, dest)
                shutil.chown(dest, user='wt-receiver', group='wt-receiver')
        run('systemctl', 'start', 'wt-receiver')
        health()
    raise
