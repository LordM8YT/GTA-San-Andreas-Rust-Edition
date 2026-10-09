"""Build a review-only binary ZIP from an explicit allowlist; never package a game tree."""
import argparse
import json
from pathlib import Path
import subprocess
import zipfile
import urllib.request
import urllib.error
import re
import uuid
import hashlib
import os
from concurrent.futures import ThreadPoolExecutor

REPO = Path(__file__).resolve().parents[1]

# Keep aligned with safe_path in native/crates/launcher/src/updater.rs: a package
# containing any other name is rejected by every installed client's updater.
DEVICES = {'CON', 'PRN', 'AUX', 'NUL'} | {f'{prefix}{index}' for prefix in ('COM', 'LPT') for index in range(1, 10)}

def updater_part(part):
    """Map one dependency notice name component onto the updater's path alphabet."""
    part = re.sub(r'[^A-Za-z0-9._+ -]', '_', part).rstrip('. ') or '_'
    if part in ('.', '..') or part.split('.')[0].upper() in DEVICES:
        part = '_' + part
    return part

def updater_accepts(name):
    return 0 < len(name) < 240 and all(
        part and part not in ('.', '..') and part.split('.')[0].upper() not in DEVICES
        and not part.endswith(('.', ' ')) and re.fullmatch(r'[A-Za-z0-9._+ -]+', part)
        for part in name.split('/'))

def upstream_notices(dependency, cache):
    """License text only, pinned to the registry package's source commit. Never executed."""
    root = Path(dependency['manifest_path']).parent
    try:
        vcs = json.loads((root / '.cargo_vcs_info.json').read_text(encoding='utf-8'))
        commit = vcs['git']['sha1']
    except (OSError, ValueError, KeyError):
        return []
    repository = dependency.get('repository') or ''
    match = re.fullmatch(r'https://(github\.com|gitlab\.com)/([A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+?)(?:\.git)?(?:/tree/[^?#]+)?/?', repository)
    if not match or not re.fullmatch(r'[0-9a-f]{40}', commit):
        return []
    base = (f'https://raw.githubusercontent.com/{match[2]}/{commit}/' if match[1] == 'github.com'
            else f'https://gitlab.com/{match[2]}/-/raw/{commit}/')
    cache_root = cache / (match[2].replace('/', '_') + '-' + commit)
    cache_root.mkdir(parents=True, exist_ok=True)
    def fetch(name):
        path = cache_root / name
        if path.is_file():
            return path
        missing = path.with_name(path.name + ".absent")
        if missing.is_file():
            return None
        try:
            with urllib.request.urlopen(base + name, timeout=10) as response:
                data = response.read(1024 * 1024 + 1)
            if len(data) > 1024 * 1024 or not data.strip():
                return None
            data.decode('utf-8')
            path.write_bytes(data)
            return path
        except urllib.error.HTTPError as error:
            if error.code == 404:
                missing.write_text("Not found at pinned commit", encoding="utf-8")
            return None
        except (OSError, UnicodeError):
            return None
    names = ['LICENSE', 'LICENSE-MIT', 'LICENSE-APACHE', 'LICENSE.md', 'LICENSE.txt', 'LICENSE_APACHE', 'LICENSE_MIT', 'COPYING', 'NOTICE']
    with ThreadPoolExecutor(max_workers=4) as workers:
        return [(path, base + path.name) for path in workers.map(fetch, names) if path is not None]

def package(target: Path, destination: Path, platform: str) -> Path:
    suffix = '.exe' if platform == 'windows' else ''
    binaries = [target / (name + suffix) for name in ('sa-launcher', 'sa-runtime', 'sa-server', 'sa-relay')]
    for binary in binaries:
        if not binary.is_file() or binary.is_symlink():
            raise ValueError(f'Missing or unsafe binary: {binary.name}')
    metadata = json.loads(subprocess.check_output([
        'cargo', 'metadata', '--locked', '--format-version', '1', '--filter-platform', 'x86_64-pc-windows-msvc' if platform == 'windows' else 'x86_64-unknown-linux-gnu', '--manifest-path', str(REPO / 'native/Cargo.toml')
    ], cwd=REPO))
    destination.mkdir(parents=True, exist_ok=True)
    output = destination / f'SARE-{platform}-test.zip'
    staging = destination / (output.name + '.staging-' + uuid.uuid4().hex)
    missing_notices = []
    notices = ['SARE test package. Review before redistribution.', 'Dependency license notices follow; no original game files are included.']
    with zipfile.ZipFile(staging, 'w', compression=zipfile.ZIP_DEFLATED, strict_timestamps=False) as archive:
        for binary in binaries:
            archive.write(binary, binary.name)
        for source, name in [
            (REPO / 'LICENSE', 'LICENSE'),
            (REPO / 'docs/client-package.md', 'START-HERE.md'),
            (REPO / 'native/Cargo.lock', 'Cargo.lock'),
            (REPO / 'native/crates/runtime/assets/UnifrakturCook-OFL.txt', 'licenses/UnifrakturCook-OFL.txt'),
            (REPO / 'native/crates/runtime/vendor/fsr1/license.txt', 'licenses/FSR1.txt'),
        ]:
            archive.write(source, name)
        for dependency in metadata['packages']:
            if dependency['source'] is None:
                continue
            identifier = dependency['name'] + '-' + dependency['version']
            notices.append(f"{identifier}: {dependency.get('license') or 'see bundled notice'}; {dependency.get('repository') or 'https://crates.io/crates/' + dependency['name']}")
            root = Path(dependency['manifest_path']).parent
            files = [p for p in root.iterdir() if p.is_file() and not p.is_symlink() and p.name.upper().startswith(('LICENSE', 'LICENCE', 'COPYING', 'NOTICE'))]
            if dependency.get('license_file'):
                explicit = (root / dependency['license_file']).resolve()
                if explicit.is_relative_to(root.resolve()) and explicit.is_file() and explicit not in files:
                    files.append(explicit)
            if not files:
                for file, source in upstream_notices(dependency, REPO / 'native/target/license-cache'):
                    files.append(file)
                    notices.append('  Pinned notice source: ' + source)
            if dependency['name'] == 'epaint_default_fonts':
                files.extend((root / 'fonts').glob('*.txt'))
            written = set()
            for file in files:
                name = f'licenses/{updater_part(identifier)}/{updater_part(file.name)}'
                if name.lower() in written:
                    continue
                written.add(name.lower())
                archive.write(file, name)
            if not files:
                missing_notices.append(identifier)
                notices.append('  No standalone notice in the registry package; consult the source license before public redistribution.')
        archive.writestr('THIRD-PARTY-NOTICES.txt', '\n'.join(notices) + '\n')
    if missing_notices:
        staging.unlink()
        raise ValueError("Required dependency notices unavailable: " + ", ".join(missing_notices))
    commit = os.environ.get('GITHUB_SHA') or subprocess.run(['git', 'rev-parse', 'HEAD'], cwd=Path(__file__).resolve().parents[1], capture_output=True, text=True, check=True).stdout.strip()
    if not re.fullmatch(r'[0-9a-f]{40}', commit):
        staging.unlink()
        raise ValueError('Invalid package commit')
    with zipfile.ZipFile(staging, 'a', compression=zipfile.ZIP_DEFLATED) as archive:
        rejected = [name for name in archive.namelist() if not updater_accepts(name)]
        if rejected:
            archive.close()
            staging.unlink()
            raise ValueError('Package paths the client updater would reject: ' + ', '.join(rejected[:8]))
        files = [{'path': name, 'size': archive.getinfo(name).file_size, 'sha256': hashlib.sha256(archive.read(name)).hexdigest()} for name in archive.namelist()]
        archive.writestr('sare-build.json', json.dumps({'schema': 1, 'commit': commit, 'platform': platform, 'files': files}, indent=2))
    staging.replace(output)
    return output

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--platform', choices=['windows', 'linux'], required=True)
    parser.add_argument('--target', type=Path, default=REPO / 'native/target/release')
    parser.add_argument('--output', type=Path, default=REPO / 'native/target/packages')
    args = parser.parse_args()
    print(package(args.target, args.output, args.platform))
