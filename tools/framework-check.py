"""Report which FiveM runtime features a resource's server scripts need from SARE.

Usage: python tools/framework-check.py <resource-or-resources-folder> [--all] [--json]

Reads each fxmanifest.lua, collects the server-side Lua it would run (server and
shared scripts, `@other/file.lua` includes and non-client Lua listed under
`files`), and compares the globals those scripts call with what sa-server
provides (native/crates/server/src/natives.rs and prelude.lua). This is a
static estimate: a resource with no missing natives can still fail at runtime,
and a missing name may be a global defined by another resource.
"""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
SERVER_SRC = REPO / 'native' / 'crates' / 'server' / 'src'
SCRIPT_KEYS = {
    'server_script': 'server', 'server_scripts': 'server',
    'shared_script': 'shared', 'shared_scripts': 'shared',
    'client_script': 'client', 'client_scripts': 'client',
    'file': 'file', 'files': 'file',
    'dependency': 'dependency', 'dependencies': 'dependency',
    'provide': 'provide', 'ui_page': 'ui_page', 'lua54': 'lua54',
}
# Runtime features found by pattern, with SARE's status.
FEATURES = [
    ('state bags', r'\b(GlobalState|Player\s*\([^)]*\)\s*\.\s*state|Entity\s*\([^)]*\)\s*\.\s*state)',
     'partial: server-side only, not replicated to clients'),
    ('function references', r'__cfx_functionReference', 'supported between server resources'),
    ('msgpack', r'\bmsgpack\.', 'supported (values pass through JSON)'),
    ('CfxLua syntax', r'`[^`\n]+`|\w\?[.\[]|\s(\+|-|\*|/|<<|>>|&|\||\^)=\s', 'supported: translated to Lua 5.4'),
    ('glm math library', r'\bglm\.', 'missing'),
    ('Citizen.InvokeNative', r'Citizen\.InvokeNative', 'missing'),
    ('playerConnecting deferrals', r'deferrals\.', 'partial: the player is already connected'),
    ('license identifiers', r"['\"](license2?|discord|steam|fivem|xbl|live)['\"]", 'missing: only sare:<id> exists'),
    ('routing buckets', r'RoutingBucket', 'missing'),
    ('OneSync entities', r'\b(CreateVehicle|CreatePed|CreateObject|NetworkGetEntityFromNetworkId|GetGamePool)\b', 'missing'),
]


def provided_names() -> set[str]:
    natives = (SERVER_SRC / 'natives.rs').read_text(encoding='utf-8')
    prelude = (SERVER_SRC / 'prelude.lua').read_text(encoding='utf-8')
    names = set(re.findall(r'func!\(\s*g,\s*"(\w+)"', natives))
    names |= set(re.findall(r'g\.set\("(\w+)"', natives))
    names |= set(re.findall(r'^function\s+([A-Za-z_][\w.]*)\s*\(', prelude, re.M))
    for line in re.findall(r'^([A-Za-z_][\w, .]*?)\s*=', prelude, re.M):
        names |= {n.strip() for n in line.split(',')}
    return {n for n in names if n and not n.startswith('__')}


def strip_comments(text: str) -> str:
    text = re.sub(r'--\[(=*)\[.*?\]\1\]', '', text, flags=re.S)
    return re.sub(r'--[^\n]*', '', text)


def read_manifest(folder: Path) -> dict[str, list[str]]:
    """Collect quoted values per known key; tolerant of FiveM manifest syntax."""
    path = next((folder / f for f in ('fxmanifest.lua', '__resource.lua') if (folder / f).is_file()), None)
    out: dict[str, list[str]] = {}
    if not path:
        return out
    tokens = re.findall(r"[A-Za-z_]\w*|'[^']*'|\"[^\"]*\"|[{}()]", strip_comments(path.read_text(encoding='utf-8')))
    key, depth = None, 0
    for token in tokens:
        if token in '{(':
            depth += 1
        elif token in '})':
            depth = max(0, depth - 1)
        elif token[0] in '\'"':
            if key:
                out.setdefault(key, []).append(token[1:-1])
        elif depth == 0:
            key = SCRIPT_KEYS.get(token)
    return out


def glob_regex(pattern: str) -> str:
    """`*` within one folder, `**/` across zero or more folders, as sa-server."""
    out, i = '', 0
    while i < len(pattern):
        if pattern.startswith('**/', i):
            out, i = out + '(?:.*/)?', i + 3
        elif pattern.startswith('**', i):
            out, i = out + '.*', i + 2
        elif pattern[i] == '*':
            out, i = out + '[^/]*', i + 1
        else:
            out, i = out + re.escape(pattern[i]), i + 1
    return out


def expand(folder: Path, pattern: str) -> list[Path]:
    pattern = pattern.replace('\\', '/')
    if '*' not in pattern:
        path = folder / pattern
        return [path] if path.is_file() else []
    regex = glob_regex(pattern)
    return sorted(p for p in folder.rglob('*') if p.is_file() and re.fullmatch(regex, p.relative_to(folder).as_posix()))


def discover(root: Path) -> dict[str, Path]:
    if (root / 'fxmanifest.lua').is_file() or (root / '__resource.lua').is_file():
        return {root.name: root}
    found = {}
    for manifest in sorted(root.rglob('fxmanifest.lua')):
        found.setdefault(manifest.parent.name, manifest.parent)
    return found


def label(path: Path, resources: dict[str, Path]) -> str:
    for name, folder in resources.items():
        if folder in path.parents:
            return f'{name}/{path.relative_to(folder).as_posix()}'
    return path.as_posix()


def check(name: str, folder: Path, resources: dict[str, Path], provided: set[str],
          scan_all: bool = False) -> dict:
    manifest = read_manifest(folder)
    files, notes = [], []
    for kind in ('shared', 'server'):
        for pattern in manifest.get(kind, []):
            if pattern.startswith('@'):
                other, _, rel = pattern[1:].partition('/')
                if other in resources:
                    files += expand(resources[other], rel)
                else:
                    notes.append(f'include {pattern}: resource {other} not in the scanned folder')
            else:
                files += expand(folder, pattern)
    # Lazily loaded modules (LoadResourceFile + load) are listed under files.
    for pattern in manifest.get('file', []):
        if pattern.endswith('.lua') and ('server' in pattern or 'shared' in pattern):
            files += expand(folder, pattern)
    if scan_all:
        files += [p for p in sorted(folder.rglob('*.lua'))
                  if 'client' not in p.relative_to(folder).as_posix() and p.name != 'fxmanifest.lua']
    if manifest.get('client'):
        notes.append(f"{len(manifest['client'])} client script entries: not run (no client Lua yet)")
    if manifest.get('ui_page'):
        notes.append('ui_page (NUI): not supported')
    for pattern in manifest.get('server', []):
        if pattern.endswith('.js'):
            notes.append(f'JavaScript server script {pattern}: not supported (no Node runtime)')
    dependencies = manifest.get('dependency', [])
    for dep in dependencies:
        if dep == '/onesync':
            notes.append('requires OneSync: SARE has no server-side entities yet')
        elif dep.startswith('/'):
            notes.append(f'version constraint {dep}: ignored')

    defined, used, features = set(), {}, {}
    seen = set()
    for path in files:
        if path in seen or not path.is_file():
            continue
        seen.add(path)
        text = strip_comments(path.read_text(encoding='utf-8', errors='replace'))
        code = re.sub(r"'(?:\\.|[^'\\\n])*'|\"(?:\\.|[^\"\\\n])*\"|\[(=*)\[.*?\]\1\]", "''", text, flags=re.S)
        defined |= set(re.findall(r'function\s+([A-Z]\w*)\s*\(', code))
        defined |= set(re.findall(r'(?:local\s+)?([A-Z]\w*)\s*=(?!=)', code))
        for call in re.findall(r'(?<![\w.:])([A-Z]\w*)\s*\(', code):
            used[call] = used.get(call, 0) + 1
        for feature, pattern, status in FEATURES:
            if re.search(pattern, text):
                features[feature] = status
    missing = {k: v for k, v in used.items() if k not in defined and k not in provided}
    supported = {k: v for k, v in used.items() if k in provided}
    return {
        'resource': name,
        'scripts': [label(p, resources) for p in sorted(seen)],
        'dependencies': dependencies,
        'provides': manifest.get('provide', []),
        'supported': dict(sorted(supported.items(), key=lambda x: (-x[1], x[0]))),
        'missing': dict(sorted(missing.items(), key=lambda x: (-x[1], x[0]))),
        'features': features,
        'notes': notes,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('folder', type=Path)
    parser.add_argument('--json', action='store_true')
    parser.add_argument('--all', action='store_true',
                        help='also scan every Lua file outside client folders (lazily loaded modules)')
    args = parser.parse_args()
    resources = discover(args.folder)
    if not resources:
        print(f'No fxmanifest.lua under {args.folder}', file=sys.stderr)
        return 1
    provided = provided_names()
    reports = [check(n, f, resources, provided, args.all) for n, f in resources.items()]
    if args.json:
        print(json.dumps(reports, indent=2))
        return 0
    for r in reports:
        print(f"== {r['resource']}: {len(r['scripts'])} server-side Lua files, "
              f"{len(r['supported'])} SARE globals used, {len(r['missing'])} missing")
        if r['missing']:
            print('  missing:', ', '.join(f'{k} ({v})' for k, v in r['missing'].items()))
        for feature, status in r['features'].items():
            print(f'  {feature}: {status}')
        for note in r['notes']:
            print(f'  note: {note}')
    return 0


if __name__ == '__main__':
    sys.exit(main())
