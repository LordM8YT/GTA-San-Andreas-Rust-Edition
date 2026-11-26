"""Build the Windows installer (SARE-Setup.exe) from the tested client ZIP.

The installer holds exactly the files of SARE-windows-test.zip, so the
installed folder is a normal packaged client and the launcher's own updater
keeps it up to date afterwards. Needs Inno Setup 6 (ISCC.exe) on Windows.

    python tools/build-installer.py                 # build
    python tools/build-installer.py --smoke-test    # build, then silently
                                                    # install, verify, uninstall
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import zipfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
SCRIPT = REPO / 'tools/installer/sare.iss'
PACKAGES = REPO / 'native/target/packages'


def iscc() -> Path:
    candidates = [os.environ.get('ISCC', '')]
    for root in (os.environ.get('ProgramFiles(x86)'), os.environ.get('ProgramFiles'), os.environ.get('LOCALAPPDATA')):
        if root:
            candidates += [Path(root) / 'Inno Setup 6/ISCC.exe', Path(root) / 'Programs/Inno Setup 6/ISCC.exe']
    candidates.append(shutil.which('iscc') or '')
    for candidate in candidates:
        if candidate and Path(candidate).is_file():
            return Path(candidate)
    raise SystemExit('Inno Setup 6 (ISCC.exe) not found; install it or set ISCC')


def workspace_version() -> str:
    text = (REPO / 'native/Cargo.toml').read_text(encoding='utf-8')
    match = re.search(r'^version\s*=\s*"(\d+)\.(\d+)\.(\d+)"', text, re.M)
    if not match:
        raise SystemExit('No workspace version in native/Cargo.toml')
    return '.'.join(match.groups())


def build(package: Path, output: Path) -> Path:
    with zipfile.ZipFile(package) as archive:
        build_info = json.loads(archive.read('sare-build.json'))
        if build_info.get('platform') != 'windows':
            raise SystemExit(f'{package.name} is not a Windows client package')
        with tempfile.TemporaryDirectory(prefix='sare-installer-') as staging:
            archive.extractall(staging)
            version = workspace_version()
            output.mkdir(parents=True, exist_ok=True)
            subprocess.run([
                str(iscc()), '/Q',
                f'/DSourceDir={staging}',
                f'/DOutputDir={output.resolve()}',
                f'/DAppVersion={version} ({build_info["commit"][:7]})',
                f'/DNumericVersion={version}.0',
                str(SCRIPT),
            ], check=True)
    return output / 'SARE-Setup.exe'


def verify_install(root: Path) -> None:
    """Every file of sare-build.json is installed byte for byte."""
    build_info = json.loads((root / 'sare-build.json').read_text(encoding='utf-8'))
    for entry in build_info['files']:
        path = root / entry['path']
        data = path.read_bytes()
        if len(data) != entry['size'] or hashlib.sha256(data).hexdigest() != entry['sha256']:
            raise SystemExit(f'Installed file differs from the package: {entry["path"]}')
    print(f'Installed {len(build_info["files"])} files, all match the package')


def wait_until(condition, what: str, seconds: float = 120) -> None:
    deadline = time.monotonic() + seconds
    while not condition():
        if time.monotonic() > deadline:
            raise SystemExit(f'Timed out waiting for {what}')
        time.sleep(1)


def smoke_test(installer: Path) -> None:
    """Silent install into a temporary folder, verify, then uninstall."""
    root = Path(tempfile.mkdtemp(prefix='sare-install-')) / 'SARE'
    subprocess.run([str(installer), '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', f'/DIR={root}'], check=True)
    wait_until(lambda: (root / 'unins000.exe').is_file(), 'the installer to finish')
    verify_install(root)
    # A player's own world and mod must survive an uninstall.
    (root / 'content/my-world').mkdir(parents=True, exist_ok=True)
    (root / 'content/my-world/content.json').write_text('{}', encoding='utf-8')
    (root / 'mods/my-mod').mkdir(parents=True, exist_ok=True)
    (root / 'mods/my-mod/fxmanifest.lua').write_text('', encoding='utf-8')
    (root / '.sare-update').mkdir(exist_ok=True)
    (root / 'sare-runtime.log').write_text('log', encoding='utf-8')
    subprocess.run([str(root / 'unins000.exe'), '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART'], check=True)
    # The uninstaller hands over to a copy of itself and returns at once.
    expected = ['content/my-world/content.json', 'mods/my-mod/fxmanifest.lua']
    def left():
        return sorted(p.relative_to(root).as_posix() for p in root.rglob('*') if p.is_file())
    try:
        wait_until(lambda: left() == expected, 'the uninstaller')
    except SystemExit:
        raise SystemExit(f'Uninstall left {left()}, expected only {expected}')
    print('Uninstall removed SARE and kept the player\'s own worlds and mods')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--package', type=Path, default=PACKAGES / 'SARE-windows-test.zip')
    parser.add_argument('--output', type=Path, default=PACKAGES)
    parser.add_argument('--smoke-test', action='store_true')
    args = parser.parse_args()
    if sys.platform != 'win32':
        raise SystemExit('The installer is built on Windows')
    result = build(args.package, args.output)
    print(result)
    if args.smoke_test:
        smoke_test(result)
