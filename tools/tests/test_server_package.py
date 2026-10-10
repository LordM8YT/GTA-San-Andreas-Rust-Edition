import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import zipfile

spec = importlib.util.spec_from_file_location('package_server', Path(__file__).parents[1] / 'package-server.py')
package_server = importlib.util.module_from_spec(spec)
spec.loader.exec_module(package_server)

class ServerPackageTests(unittest.TestCase):
    def build(self, platform):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        for path in ['server-data/server.cfg', 'server-data/resources/[managers]/mapmanager/fxmanifest.lua',
                     'server-data/resources/[local]/.gitkeep', 'server-data/server.log', 'docs/server-package.md']:
            file = root / path
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text('owned test support file', encoding='utf-8')
        target = root / 'build'
        target.mkdir()
        suffix = '.exe' if platform == 'windows' else ''
        for name in ['sa-server', 'sa-relay', 'sa-launcher', 'sa-runtime']:
            (target / (name + suffix)).write_bytes(b'owned test fixture')
        client = root / 'client.zip'
        with zipfile.ZipFile(client, 'w') as archive:
            for name in ['LICENSE', 'THIRD-PARTY-NOTICES.txt', 'licenses/x-1.0/LICENSE', 'sa-launcher' + suffix]:
                archive.writestr(name, 'notice')
        with patch.object(package_server, 'REPO', root):
            return package_server.package(target, client, root / 'output', platform)

    def test_fivem_style_layout_with_only_server_binaries(self):
        with zipfile.ZipFile(self.build('windows')) as archive:
            names = set(archive.namelist())
        self.assertEqual({n for n in names if n.endswith('.exe')}, {'server/sa-server.exe', 'server/sa-relay.exe'})
        for expected in ['server-data/server.cfg', 'server-data/resources/[managers]/mapmanager/fxmanifest.lua',
                         'server-data/resources/[local]/.gitkeep', 'server/LICENSE', 'server/THIRD-PARTY-NOTICES.txt',
                         'server/licenses/x-1.0/LICENSE', 'start-server.cmd', 'START-HERE.md']:
            self.assertIn(expected, names)
        self.assertNotIn('server-data/server.log', names)

    def test_linux_scripts_and_binaries_are_executable(self):
        with zipfile.ZipFile(self.build('linux')) as archive:
            for name in ['server/sa-server', 'server/sa-relay', 'start-server.sh', 'start-relay.sh']:
                self.assertEqual(archive.getinfo(name).external_attr >> 16, 0o755, name)

if __name__ == '__main__':
    unittest.main()
