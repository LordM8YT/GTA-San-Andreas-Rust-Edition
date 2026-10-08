import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

spec = importlib.util.spec_from_file_location('package_client', Path(__file__).parents[1] / 'package-client.py')
package_client = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package_client)

class ClientPackageTests(unittest.TestCase):
    def test_explicit_allowlist_excludes_assets_settings_cache_and_logs(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for path in ['LICENSE','docs/client-package.md','native/Cargo.lock','native/crates/runtime/assets/UnifrakturCook-OFL.txt','native/crates/runtime/vendor/fsr1/license.txt']:
                file = root / path
                file.parent.mkdir(parents=True, exist_ok=True)
                file.write_text('owned test support file', encoding='utf-8')
            target = root / 'build'
            target.mkdir()
            for name in ['sa-launcher.exe','sa-runtime.exe','sa-server.exe','sa-relay.exe','gta3.img','settings.json','private.log','cache.bin']:
                (target / name).write_bytes(b'owned test fixture')
            # Neither project-owned examples nor private imports belong in an
            # installation ZIP, even if they sit beside the binaries.
            mod_payload = b'PRIVATE MOD PAYLOAD - MUST NEVER BE DISTRIBUTED'
            for parent in [root, target]:
                for path in ['mods/native-car-demo/car.dff',
                             'mods/local-fivem-test/imported.yft',
                             'mods/local-upscaled-textures/road.png',
                             'private-assets/original.txd',
                             'server-cache/session/resource.json']:
                    fixture = parent / path
                    fixture.parent.mkdir(parents=True, exist_ok=True)
                    fixture.write_bytes(mod_payload)
            with patch.object(package_client, 'REPO', root), patch.object(package_client.subprocess, 'check_output', return_value=b'{"packages":[]}'):
                output = package_client.package(target, root / 'output', 'windows')
            with zipfile.ZipFile(output) as archive:
                names = set(archive.namelist())
                self.assertEqual({name for name in names if name.endswith('.exe')}, {'sa-launcher.exe','sa-runtime.exe','sa-server.exe','sa-relay.exe'})
                for forbidden in ['gta3.img','settings.json','private.log','cache.bin']:
                    self.assertNotIn(forbidden, names)
                self.assertFalse(any(name.split('/')[0] in {'mods', 'private-assets', 'server-cache'} for name in names))
                self.assertFalse(any(mod_payload in archive.read(name) for name in names))
                self.assertIn('START-HERE.md', names)
                self.assertIn('THIRD-PARTY-NOTICES.txt', names)
                import json, hashlib
                manifest = json.loads(archive.read('sare-build.json'))
                self.assertEqual(manifest['schema'], 1)
                self.assertEqual(manifest['platform'], 'windows')
                self.assertRegex(manifest['commit'], r'^[0-9a-f]{40}$')
                self.assertEqual({entry['path'] for entry in manifest['files']}, names - {'sare-build.json'})
                for entry in manifest['files']:
                    data = archive.read(entry['path'])
                    self.assertEqual(entry['size'], len(data))
                    self.assertEqual(entry['sha256'], hashlib.sha256(data).hexdigest())
            self.assertEqual((target / 'gta3.img').read_bytes(), b'owned test fixture')
            previous = output.read_bytes()
            import json
            dependency_root = root / 'dependency'
            dependency_root.mkdir()
            incomplete = {'packages': [{'name': 'missing-notice', 'version': '1.0.0', 'source': 'registry+fixture', 'license': 'MIT', 'repository': 'https://untrusted.example', 'manifest_path': str(dependency_root / 'Cargo.toml')}]}
            with patch.object(package_client, 'REPO', root), patch.object(package_client.subprocess, 'check_output', return_value=json.dumps(incomplete).encode()):
                with self.assertRaisesRegex(ValueError, 'Required dependency notices unavailable'):
                    package_client.package(target, root / 'output', 'windows')
            self.assertEqual(output.read_bytes(), previous)
            self.assertEqual(list((root / 'output').glob('*.staging-*')), [])


    def test_missing_runtime_rejects_incomplete_package(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / 'sa-launcher.exe').write_bytes(b'owned')
            with self.assertRaises(ValueError):
                package_client.package(root, root / 'output', 'windows')
            self.assertFalse((root / 'output').exists())

    def test_upstream_license_is_pinned_and_untrusted_hosts_are_not_contacted(self):
        import io
        import json
        import urllib.error
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / 'Cargo.toml').write_text('', encoding='utf-8')
            commit = 'a' * 40
            (root / '.cargo_vcs_info.json').write_text(json.dumps({'git': {'sha1': commit}}), encoding='utf-8')
            dependency = {'manifest_path': str(root / 'Cargo.toml'), 'repository': 'https://github.com/example/project/tree/main/crates/client'}
            def response(url, **kwargs):
                self.assertTrue(url.startswith('https://raw.githubusercontent.com/example/project/' + commit + '/'))
                if url.endswith('/LICENSE-MIT'):
                    return io.BytesIO(b'owned fixture license notice')
                raise urllib.error.HTTPError(url, 404, 'fixture missing', {}, None)
            with patch.object(package_client.urllib.request, 'urlopen', side_effect=response) as request:
                files = package_client.upstream_notices(dependency, root / 'cache')
                self.assertEqual(len(files), 1)
                calls = request.call_count
                self.assertEqual(len(package_client.upstream_notices(dependency, root / 'cache')), 1)
                self.assertEqual(request.call_count, calls)
                dependency['repository'] = 'https://untrusted.example/project'
                self.assertEqual(package_client.upstream_notices(dependency, root / 'cache'), [])
                self.assertEqual(request.call_count, calls)
