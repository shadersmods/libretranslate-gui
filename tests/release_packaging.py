"""Exercise the release packager against each native Dioxus output layout."""
import hashlib
import importlib.util
from pathlib import Path
import tarfile
import tempfile
import unittest
import zipfile

SOURCE = Path(__file__).resolve().parents[1] / '.github/package_release.py'
spec = importlib.util.spec_from_file_location('release_packager', SOURCE)
packager = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packager)


class PackagingTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.target = self.root / 'build-output'
        for name in ('Cargo.toml', 'README.md', 'THIRD_PARTY_NOTICES.md', 'licenses/MIT.txt',
                     'docs/usage.md', 'docs/images/preview.png', 'locales/README.md'):
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('[package]\nversion = "0.2.7"\n' if name == 'Cargo.toml' else 'fixture')

    def app(self, system):
        output = self.target / 'dx/libretranslate-gui/release' / system
        if system == 'macos':
            bundle = output / 'LibretranslateGui.app'
            executable = bundle / 'Contents/MacOS/libretranslate-gui'
            assets = bundle / 'Contents/Resources/assets'
            executable.parent.mkdir(parents=True)
            (bundle / 'Contents/Info.plist').write_bytes(b'plist fixture')
        else:
            executable = output / 'app' / ('libretranslate-gui.exe' if system == 'windows' else 'libretranslate-gui')
            assets = executable.parent / 'assets'
        assets.mkdir(parents=True)
        (assets / 'app.css').write_text('body {}')
        executable.write_bytes(b'binary fixture')
        (executable.parent / 'data').mkdir()
        (executable.parent / 'data/instances.json').write_text('private instance data')
        (executable.parent / 'libretranslate-desktop.json').write_text('private preferences')
        return executable

    def test_all_platforms_include_program_files_docs_and_matching_checksum(self):
        for system, runner, machine, suffix in (
            ('windows', 'Windows', 'AMD64', 'Windows-x64.zip'),
            ('macos', 'Darwin', 'arm64', 'macOS-arm64.zip'),
            ('linux', 'Linux', 'x86_64', 'Linux-x64.tar.gz'),
        ):
            with self.subTest(system=system):
                self.app(system)
                archive = packager.package(self.root, system=runner, machine=machine,
                                           target=self.target, tag='v0.2.7')
                prefix = 'LibreTranslate-GUI-0.2.7-' + suffix.split('.')[0]
                self.assertEqual(archive.name, 'LibreTranslate-GUI-0.2.7-' + suffix)
                if system == 'linux':
                    with tarfile.open(archive) as bundle:
                        names = bundle.getnames()
                else:
                    with zipfile.ZipFile(archive) as bundle:
                        self.assertIsNone(bundle.testzip())
                        names = bundle.namelist()
                        if system == 'macos':
                            executable = prefix + '/LibretranslateGui.app/Contents/MacOS/libretranslate-gui'
                            self.assertIn(executable, names)
                            self.assertEqual(bundle.getinfo(executable).external_attr >> 16 & 0o777, 0o755)
                            self.assertIn(prefix + '/LibretranslateGui.app/Contents/Info.plist', names)
                            self.assertIn(prefix + '/LibretranslateGui.app/Contents/Resources/assets/app.css', names)
                self.assertIn(prefix + '/docs/usage.md', names)
                self.assertIn(prefix + '/docs/images/preview.png', names)
                self.assertFalse(any('instances.json' in name or 'libretranslate-desktop.json' in name for name in names))
                checksum = archive.with_suffix(archive.suffix + '.sha256').read_text().split()
                self.assertEqual(checksum, [hashlib.sha256(archive.read_bytes()).hexdigest(), archive.name])

    def test_incomplete_macos_bundle_is_rejected(self):
        executable = self.app('macos')
        executable.unlink()
        with self.assertRaisesRegex(SystemExit, 'complete Dioxus release app bundle'):
            packager.package(self.root, system='Darwin', machine='arm64', target=self.target, tag='v0.2.7')
        self.assertFalse((self.root / 'release').exists())

    def test_tag_must_match_the_app_version(self):
        with self.assertRaisesRegex(SystemExit, 'tag must match'):
            packager.package(self.root, tag='v0.2.8')


if __name__ == '__main__':
    unittest.main()
