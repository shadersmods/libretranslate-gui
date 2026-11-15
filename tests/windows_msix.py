"""Validate MSIX identity, desktop activation, icons and private-data exclusion."""
import importlib.util
from pathlib import Path
import struct
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('build_msix', ROOT / '.github/build_msix.py')
msix = importlib.util.module_from_spec(spec)
spec.loader.exec_module(msix)


class MsixTests(unittest.TestCase):
    def test_unsigned_identity_is_explicit_and_uses_full_trust_desktop_activation(self):
        document = msix.manifest('0.2.9')
        identity = document.find('{' + msix.FOUNDATION + '}Identity')
        self.assertEqual(identity.get('Version'), '0.2.9.0')
        self.assertEqual(identity.get('ProcessorArchitecture'), 'x64')
        self.assertTrue(identity.get('Name').endswith('.Unsigned'))
        self.assertIn('OID.2.25.311729368913984317654407730594956997722=1', identity.get('Publisher'))
        application = document.find('.//{' + msix.FOUNDATION + '}Application')
        self.assertEqual(application.get('Executable'), 'libretranslate-gui.exe')
        self.assertEqual(application.get('EntryPoint'), 'Windows.FullTrustApplication')
        self.assertEqual(document.find('.//{' + msix.RESCAP + '}Capability').get('Name'), 'runFullTrust')

    def test_payload_never_includes_profiles_or_msi_markers(self):
        with tempfile.TemporaryDirectory() as temporary:
            stage = Path(temporary)
            (stage / 'assets').mkdir()
            for name in ('libretranslate-gui.exe', 'README.md', 'THIRD_PARTY_NOTICES.md', 'assets/icon.png'):
                (stage / name).write_text('fixture')
            self.assertEqual(len(msix.payload_files(stage)), 4)
            for name in ('libretranslate-desktop.json', 'data/instances.json', 'libretranslate-gui.install.json'):
                path = stage / name
                path.parent.mkdir(exist_ok=True)
                path.write_text('private')
                with self.assertRaisesRegex(ValueError, 'Unexpected MSIX payload'):
                    msix.payload_files(stage)
                path.unlink()

    def test_required_logo_dimensions_and_version_bounds(self):
        for name, size in [('Logo44.png', 44), ('Logo150.png', 150), ('StoreLogo.png', 50)]:
            png = (ROOT / 'packaging/windows/msix' / name).read_bytes()
            self.assertEqual(png[:8], b'\x89PNG\r\n\x1a\n')
            self.assertEqual(struct.unpack('>II', png[16:24]), (size, size))
        for version in ('1.2', '1.2.3-beta', '65536.0.0'):
            with self.assertRaises(ValueError):
                msix.manifest(version)


if __name__ == '__main__':
    unittest.main()
