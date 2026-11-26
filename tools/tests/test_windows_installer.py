import importlib.util
from pathlib import Path
import re
import unittest

spec = importlib.util.spec_from_file_location('build_installer', Path(__file__).parents[1] / 'build-installer.py')
build_installer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(build_installer)


class WindowsInstallerTests(unittest.TestCase):
    def setUp(self):
        self.script = build_installer.SCRIPT.read_text(encoding='utf-8')

    def test_installs_per_user_where_the_updater_can_write(self):
        self.assertIn('PrivilegesRequired=lowest', self.script)
        self.assertIn(r'DefaultDirName={localappdata}\SARE', self.script)

    def test_upgrade_id_never_changes(self):
        self.assertIn('AppId={{A8742781-5671-4669-AFDE-323B4BCC8EAF}', self.script)

    def test_uninstall_keeps_the_players_own_worlds_and_mods(self):
        for folder in ('content', 'mods'):
            self.assertIsNone(re.search(r'filesandordirs;\s*Name:\s*"\{app\}\\' + folder, self.script))
            self.assertIn(f'Type: dirifempty; Name: "{{app}}\\{folder}"', self.script)

    def test_shortcuts_start_the_launcher(self):
        for place in ('{autoprograms}', '{autodesktop}'):
            self.assertIn(f'Name: "{place}\\SARE"; Filename: "{{app}}\\sa-launcher.exe"', self.script)

    def test_version_comes_from_the_workspace(self):
        self.assertRegex(build_installer.workspace_version(), r'^\d+\.\d+\.\d+$')


if __name__ == '__main__':
    unittest.main()
