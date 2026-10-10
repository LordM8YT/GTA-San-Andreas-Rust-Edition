import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('framework_check', Path(__file__).parents[1] / 'framework-check.py')
framework_check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(framework_check)

REPO = Path(__file__).resolve().parents[2]


class FrameworkCheckTests(unittest.TestCase):
    def test_provided_names_come_from_the_server_sources(self):
        provided = framework_check.provided_names()
        for name in ['GetPlayerName', 'AddStateBagChangeHandler', 'GetPlayerIdentifierByType', 'Player',
                     'RegisterNetEvent', 'Citizen.CreateThread', 'msgpack']:
            self.assertIn(name, provided)
        self.assertNotIn('CreateVehicle', provided)

    def test_shipped_examples_need_nothing_missing(self):
        resources = framework_check.discover(REPO / 'server-data' / 'resources')
        provided = framework_check.provided_names()
        for name in ['sare_lib', 'sare_core', 'sare_jobs']:
            report = framework_check.check(name, resources[name], resources, provided)
            self.assertEqual(report['missing'], {}, name)
        jobs = framework_check.check('sare_jobs', resources['sare_jobs'], resources, provided)
        self.assertIn('sare_lib/init.lua', jobs['scripts'])
        self.assertIn('state bags', jobs['features'])
        self.assertEqual(framework_check.check('sare_core', resources['sare_core'], resources,
                                               provided)['provides'], ['sare-core'])

    def test_reports_missing_natives_client_scripts_and_cfxlua(self):
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary) / 'fw'
            (folder / 'server').mkdir(parents=True)
            (folder / 'fxmanifest.lua').write_text(
                "fx_version 'cerulean'\ngame 'gta5'\n-- server_script 'ignored.lua'\n"
                "shared_script '@ox_lib/init.lua'\nserver_scripts { 'server/*.lua', 'dist/app.js' }\n"
                "client_script 'client.lua'\ndependencies { '/onesync', 'ox_lib' }\nprovide 'qb-core'\n",
                encoding='utf-8')
            (folder / 'server' / 'main.lua').write_text(
                "local function Helper() end\nHelper()\nCreateVehicle(`adder`, 0, 0, 0, 0, true, true)\n"
                "print('GetGamePool(not a call)')\nlocal p = Player(source).state\nGetPlayerName(source)\n",
                encoding='utf-8')
            report = framework_check.check('fw', folder, {'fw': folder}, framework_check.provided_names())
        self.assertEqual(report['missing'], {'CreateVehicle': 1})
        self.assertEqual(report['supported'], {'GetPlayerName': 1, 'Player': 1})
        self.assertEqual(report['provides'], ['qb-core'])
        self.assertIn('CfxLua syntax', report['features'])
        notes = ' '.join(report['notes'])
        for expected in ['@ox_lib/init.lua', 'client script', 'JavaScript', 'OneSync']:
            self.assertIn(expected, notes)


if __name__ == '__main__':
    unittest.main()
