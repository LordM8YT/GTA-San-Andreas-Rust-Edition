"""Build a FiveM-style dedicated server ZIP: server/ binaries plus server-data/ resources."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import zipfile

REPO = Path(__file__).resolve().parents[1]

# Same split as FXServer: replace server/ to update, keep your own server-data/.
# Exit code 42 means sa-server installed an update from GitHub: start it again.
WINDOWS_START = '''@echo off
rem Runs sa-server inside server-data, like FXServer.exe +exec server.cfg.
cd /d "%~dp0server-data"
set SARE_SERVER_SUPERVISED=1
:start
"%~dp0server\\sa-server.exe" +exec server.cfg %*
if %errorlevel%==42 goto start
if errorlevel 1 pause
'''
LINUX_START = '''#!/usr/bin/env bash
# Runs sa-server inside server-data, like FXServer +exec server.cfg.
set -uo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/server-data"
export SARE_SERVER_SUPERVISED=1
while true; do
    ../server/sa-server +exec server.cfg "$@"
    code=$?
    [[ $code -eq 42 ]] || exit $code
done
'''
WINDOWS_RELAY = '''@echo off
"%~dp0server\\sa-relay.exe" %*
if errorlevel 1 pause
'''
LINUX_RELAY = '''#!/usr/bin/env bash
set -euo pipefail
exec "$(dirname -- "${BASH_SOURCE[0]}")/server/sa-relay" "$@"
'''
SERVER_DATA_SUFFIXES = ('.cfg', '.lua', '.gitkeep')

def write(archive, name, data, executable=False):
    info = zipfile.ZipInfo(name, date_time=(2026, 1, 1, 0, 0, 0))
    info.compress_type = zipfile.ZIP_DEFLATED
    info.external_attr = (0o755 if executable else 0o644) << 16
    archive.writestr(info, data)

def commit():
    value = os.environ.get('GITHUB_SHA') or subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=REPO, capture_output=True, text=True, check=True).stdout.strip()
    if not re.fullmatch(r'[0-9a-f]{40}', value):
        raise ValueError('Invalid package commit')
    return value

def package(target: Path, client_package: Path, destination: Path, platform: str) -> Path:
    suffix = '.exe' if platform == 'windows' else ''
    binaries = [target / (name + suffix) for name in ('sa-server', 'sa-relay')]
    for binary in binaries:
        if not binary.is_file() or binary.is_symlink():
            raise ValueError(f'Missing or unsafe binary: {binary.name}')
    data_root = REPO / 'server-data'
    # No server.cfg: sa-server asks for a template (Freeroam or SARE Box) on
    # the first start, and an extracted update never overwrites the owner's.
    data = sorted(p for p in data_root.rglob('*') if p.is_file() and not p.is_symlink()
                  and p.name.endswith(SERVER_DATA_SUFFIXES)
                  and p != data_root / 'server.cfg' and 'templates' not in p.relative_to(data_root).parts)
    if not (data_root / 'resources').is_dir():
        raise ValueError('server-data/resources is missing')
    destination.mkdir(parents=True, exist_ok=True)
    output = destination / f'SARE-server-{platform}.zip'
    staging = output.with_name(output.name + '.staging')
    with zipfile.ZipFile(staging, 'w') as archive, zipfile.ZipFile(client_package) as client:
        for binary in binaries:
            write(archive, 'server/' + binary.name, binary.read_bytes(), executable=True)
        # The client package already carries the workspace's license notices.
        for name in client.namelist():
            if name in ('LICENSE', 'THIRD-PARTY-NOTICES.txt') or name.startswith('licenses/'):
                write(archive, 'server/' + name, client.read(name))
        # Manifest for the server's self-updater: every file under server/.
        files = [{'path': name[len('server/'):], 'size': archive.getinfo(name).file_size,
                  'sha256': hashlib.sha256(archive.read(name)).hexdigest()}
                 for name in archive.namelist() if name.startswith('server/')]
        write(archive, 'server/sare-build.json', json.dumps({'schema': 1, 'commit': commit(), 'platform': platform, 'files': files}, indent=2))
        for file in data:
            write(archive, file.relative_to(REPO).as_posix(), file.read_bytes())
        write(archive, 'START-HERE.md', (REPO / 'docs/server-package.md').read_bytes())
        if platform == 'windows':
            write(archive, 'start-server.cmd', WINDOWS_START.replace('\n', '\r\n'))
            write(archive, 'start-relay.cmd', WINDOWS_RELAY.replace('\n', '\r\n'))
        else:
            write(archive, 'start-server.sh', LINUX_START, executable=True)
            write(archive, 'start-relay.sh', LINUX_RELAY, executable=True)
    staging.replace(output)
    return output

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--platform', choices=['windows', 'linux'], required=True)
    parser.add_argument('--target', type=Path, default=REPO / 'native/target/release')
    parser.add_argument('--output', type=Path, default=REPO / 'native/target/packages')
    parser.add_argument('--client-package', type=Path, help='Client ZIP to copy license notices from')
    args = parser.parse_args()
    client_package = args.client_package or args.output / f'SARE-{args.platform}-test.zip'
    print(package(args.target, client_package, args.output, args.platform))
