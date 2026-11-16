"""Installer authoring regressions; --smoke also installs, upgrades and uninstalls on Windows."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import uuid

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('build_msi', ROOT / '.github/build_msi.py')
msi = importlib.util.module_from_spec(spec)
spec.loader.exec_module(msi)
NS = {'w': msi.NS}


def fixture(stage, asset='old.css'):
    (stage / 'assets').mkdir(parents=True)
    for filename in ('libretranslate-gui.exe', 'README.md', 'THIRD_PARTY_NOTICES.md', 'assets/' + asset):
        (stage / filename).write_bytes(b'installer regression fixture')


class InstallerTests(unittest.TestCase):
    def test_components_and_upgrade_identity_are_stable_and_data_is_not_owned(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            stage = root / 'stage'
            fixture(stage)
            first = msi.generate(stage, root / 'v1/Installer.wxs', '0.2.8', ROOT / 'assets/icon.ico')
            second = msi.generate(stage, root / 'v2/Installer.wxs', '0.2.9', ROOT / 'assets/icon.ico')
            one, two = first.find('w:Package', NS), second.find('w:Package', NS)
            self.assertEqual(one.get('Scope'), 'perUser')
            self.assertEqual(one.get('UpgradeCode'), two.get('UpgradeCode'))
            self.assertNotEqual(one.get('ProductCode'), two.get('ProductCode'))
            self.assertEqual([(c.get('Id'), c.get('Guid')) for c in first.findall('.//w:Component', NS)],
                             [(c.get('Id'), c.get('Guid')) for c in second.findall('.//w:Component', NS)])
            names = {f.get('Name') for f in first.findall('.//w:File', NS)}
            self.assertEqual(names, {'libretranslate-gui.exe', 'README.md', 'THIRD_PARTY_NOTICES.md', 'old.css', msi.MARKER, 'SetupUI.wxs'})
            self.assertFalse((stage / msi.MARKER).exists(), 'Portable staging must remain portable')
            self.assertIsNotNone(first.find('.//w:Shortcut', NS))
            self.assertEqual(first.find('.//w:MajorUpgrade', NS).get('Schedule'), 'afterInstallInitialize')
            self.assertIsNone(first.find('.//w:RemoveFile', NS), 'Never recursively delete unknown user files')

    def test_private_or_incomplete_payload_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            stage = root / 'stage'
            fixture(stage)
            for filename in ('libretranslate-desktop.json', 'instances.json', msi.MARKER):
                path = stage / filename
                path.write_text('private')
                with self.assertRaisesRegex(ValueError, 'Unexpected installer payload'):
                    msi.generate(stage, root / 'Installer.wxs', '0.2.8', ROOT / 'assets/icon.ico')
                path.unlink()
            (stage / 'libretranslate-gui.exe').unlink()
            with self.assertRaisesRegex(ValueError, 'complete Windows release'):
                msi.generate(stage, root / 'Installer.wxs', '0.2.8', ROOT / 'assets/icon.ico')

    def test_msi_version_limits(self):
        for version in ('0.2.8', '255.255.65535'):
            msi.validate_version(version)
        for version in ('0.2.8-beta', '1.2', '1.2.3.4', '256.0.0', '1.256.0', '1.2.65536', '01.2.3'):
            with self.subTest(version=version), self.assertRaises(ValueError):
                msi.validate_version(version)


def smoke():
    if os.name != 'nt':
        raise SystemExit('--smoke requires Windows')
    # A unique UpgradeCode and product name isolate this check from every real installation.
    identity = 'smoke-' + str(uuid.uuid4())
    work = ROOT / '.runtime-test' / identity
    work.mkdir(parents=True)
    install = work / 'Installed apps' / 'LibreTranslate GUI'
    user_data = work / 'user-data'
    user_data.mkdir()
    (user_data / 'instances.json').write_text('keep my instances')
    product_name = 'LibreTranslate GUI Test ' + identity[-12:]
    installer_command = Path(os.environ['SystemRoot']) / 'System32/msiexec.exe'

    def run(*arguments, log, install_folder=None):
        command = subprocess.list2cmdline([str(installer_command), *map(str, arguments), '/qn',
                                          '/norestart', '/L*v', str(work / log)])
        if install_folder is not None:
            # MSI quotes property values, unlike the CRT's whole-argument quoting.
            command += ' INSTALLFOLDER="' + str(install_folder) + '"'
        result = subprocess.run(command, timeout=180)
        if result.returncode not in (0, 3010):
            raise AssertionError(f'Windows Installer returned {result.returncode}; see {work / log}')

    installers = {}
    try:
        for version, asset in [('0.2.9', 'old.css'), ('0.2.10', 'new.css')]:
            stage = work / version / 'stage'
            fixture(stage, asset)
            installer = msi.build(ROOT, stage, version=version, identity=identity,
                                  product_name=product_name, work=work / version / 'wix')
            installers[version] = installer
            if version == '0.2.9':
                run('/i', installer, log=version + '-install.log', install_folder=install)
            else:
                staging = install / '.updates' / str(uuid.uuid4())
                staging.mkdir(parents=True)
                (staging / 'release.msi').write_bytes(installer.read_bytes())
                job = staging / 'job.json'
                job.write_text(json.dumps({
                    'root': str(install), 'staging': str(staging),
                    'executable': 'libretranslate-gui.exe', 'parent': 0,
                    'version': 'v' + version, 'files': [], 'msi': True,
                }), encoding='utf-8')
                environment = dict(os.environ, LIBRETRANSLATE_GUI_TEST_MSI_JOB=str(job))
                subprocess.run([
                    'cargo', 'test', '--locked',
                    'self_update::tests::msi_update_installs_into_paths_with_spaces',
                    '--', '--ignored', '--exact', '--nocapture',
                ], env=environment, check=True, timeout=300)
            assert (install / 'assets' / asset).is_file()
            assert (install / msi.MARKER).read_text() == msi.MARKER_CONTENT
            assert (install / 'libretranslate-gui.exe').is_file()
            shortcut = Path(os.environ['APPDATA']) / 'Microsoft/Windows/Start Menu/Programs' / (product_name + '.lnk')
            assert shortcut.is_file(), 'Missing Start menu shortcut'
            if version == '0.2.9':
                (install / 'personal.txt').write_text('keep an unrelated file')
            else:
                assert not (install / 'assets/old.css').exists(), 'Upgrade left an obsolete hashed asset'
        # An older installer must leave the newer product and its files intact.
        old = installers['0.2.9']
        result = subprocess.run([str(installer_command), '/i', str(old), '/qn', '/norestart',
                                 '/L*v', str(work / 'downgrade.log')], timeout=180)
        assert result.returncode == 1603, f'Downgrade was not blocked: {result.returncode}'
        assert (install / 'assets/new.css').is_file()
    finally:
        for version in reversed(installers):
            product = msi.guid(identity + ':x64:product:' + version)
            result = subprocess.run([
                str(installer_command), '/x', '{' + product + '}', '/qn', '/norestart',
                '/L*v', str(work / (version + '-uninstall.log')),
            ], timeout=180)
            assert result.returncode in (0, 1605, 3010), f'Test product cleanup failed: {result.returncode}'
    assert not (install / 'libretranslate-gui.exe').exists()
    assert not shortcut.exists()
    assert (install / 'personal.txt').read_text() == 'keep an unrelated file'
    assert (user_data / 'instances.json').read_text() == 'keep my instances'
    print('MSI install, upgrade, downgrade prevention, shortcut and uninstall passed:', work)


def verify_package(installer):
    installer = Path(installer).resolve()
    work = ROOT / '.runtime-test' / ('verify-msi-' + str(uuid.uuid4()))
    work.mkdir(parents=True)
    command = Path(os.environ['SystemRoot']) / 'System32/msiexec.exe'
    result = subprocess.run([str(command), '/a', str(installer), '/qn', '/norestart',
                             'TARGETDIR=' + str(work / 'image'), '/L*v', str(work / 'extract.log')], timeout=180)
    assert result.returncode in (0, 3010), f'MSI extraction failed: {result.returncode}; see {work}'
    executables = list((work / 'image').rglob('libretranslate-gui.exe'))
    assert len(executables) == 1, 'Expected exactly one app executable in the MSI'
    extracted = executables[0].parent
    stage = ROOT / 'release-staging' / installer.stem.replace('-Installer-', '-', 1)
    for path in stage.rglob('*'):
        if path.is_file():
            relative = path.relative_to(stage)
            assert (extracted / relative).read_bytes() == path.read_bytes(), f'MSI payload differs: {relative}'
    assert (extracted / msi.MARKER).read_text() == msi.MARKER_CONTENT
    assert (extracted / 'licenses/wix/SetupUI.wxs').read_bytes() == (ROOT / 'packaging/windows/SetupUI.wxs').read_bytes()
    actual = {p.relative_to(extracted).as_posix() for p in extracted.rglob('*') if p.is_file()}
    expected = {p.relative_to(stage).as_posix() for p in stage.rglob('*') if p.is_file()}
    assert actual == expected | {msi.MARKER, 'licenses/wix/SetupUI.wxs'}, 'Unexpected files in the MSI'
    print('Complete MSI payload extracted and verified:', installer.name)


if __name__ == '__main__':
    if '--smoke' in sys.argv:
        smoke()
    elif '--verify-package' in sys.argv:
        verify_package(sys.argv[sys.argv.index('--verify-package') + 1])
    else:
        unittest.main()
