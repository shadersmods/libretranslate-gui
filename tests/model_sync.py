"""Model synchronization regression tests; only the Python standard library is required."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import types
import unittest
from unittest.mock import patch
import zipfile
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import threading

SOURCE = Path(__file__).resolve().parents[1] / 'assets/update-models.py'


class SyncTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.base = Path(self.temporary.name)
        self.root = self.base / 'packages'
        self.root.mkdir()
        self.cache = self.base / 'cache'
        (self.cache / 'minisbd').mkdir(parents=True)
        self.downloads = []
        self.catalog = []
        fixture = self

        class Model:
            def __init__(self, metadata, path=None):
                self.__dict__.update(metadata)
                self.type = metadata.get('type', 'translate')
                self.package_path = path
                self.links = [f'fixture:{self.from_code}_{self.to_code}']

            def download(self):
                fixture.downloads.append((self.from_code, self.to_code))
                archive = fixture.base / (self.from_code + '_' + self.to_code + '.argosmodel')
                name = 'model-' + self.from_code + '_' + self.to_code
                with zipfile.ZipFile(archive, 'w') as output:
                    metadata = {key: getattr(self, key) for key in ['from_code', 'to_code', 'package_version', 'type']}
                    output.writestr(name + '/metadata.json', json.dumps(metadata))
                    output.writestr(name + '/model/model.bin', b'valid' if not getattr(self, 'damaged', False) else b'damaged')
                return archive

        self.Model = Model
        settings = types.ModuleType('argostranslate.settings')
        settings.downloads_dir = self.base
        settings.data_dir = self.base / 'data'
        settings.local_package_index = self.base / 'index.json'
        settings.remote_package_index = 'https://example.invalid/index.json'
        packages = types.ModuleType('argostranslate.package')
        packages.AvailablePackage = Model
        packages.argospm_package_name = lambda model: f'{model.from_code}_{model.to_code}-{model.package_version}'
        packages.get_installed_packages = lambda: [Model(json.loads((path / 'metadata.json').read_text()), path) for path in self.root.iterdir() if path.is_dir()]
        translator = types.ModuleType('argostranslate.translate')
        translator.get_installed_languages = lambda: []
        translator.get_installed_languages.cache_clear = lambda: None
        argos = types.ModuleType('argostranslate')
        argos.package, argos.settings, argos.translate = packages, settings, translator
        argos.sbd = types.SimpleNamespace(MiniSBDSentencizer=types.SimpleNamespace(
            LANGUAGE_CODE_MAPPING={'pb': 'pt', 'zh': 'zh-hans', 'zt': 'zh-hant', 'az': 'tr', 'bn': 'hi', 'eo': 'en'}))
        sbd = types.ModuleType('minisbd.models')
        sbd.cache_dir = str(self.cache / 'minisbd')
        sbd.MODELS = {'en': 'en.onnx', 'ro': 'ro.onnx', 'es': 'es.onnx'}
        sbd.REPO_URL = 'https://example.invalid/'
        minisbd = types.ModuleType('minisbd')
        minisbd.models = sbd
        minisbd.download_models = lambda codes, output: [(Path(sbd.cache_dir) / sbd.MODELS[code]).write_bytes(b'sentence model') for code in codes]
        language = types.ModuleType('libretranslate.language')
        language.iso2model = lambda codes: codes
        ct2 = types.ModuleType('ctranslate2')
        def validate(path, **_):
            if (Path(path) / 'model.bin').read_bytes() != b'valid':
                raise RuntimeError('Invalid model')
        ct2.Translator = validate
        versions = types.ModuleType('packaging.version')
        versions.Version = lambda text: tuple(int(part) for part in text.split('.'))
        modules = {'argostranslate': argos, 'argostranslate.package': packages, 'argostranslate.settings': settings, 'argostranslate.translate': translator,
                   'libretranslate': types.ModuleType('libretranslate'), 'libretranslate.language': language,
                   'minisbd': minisbd, 'minisbd.models': sbd, 'ctranslate2': ct2,
                   'packaging': types.ModuleType('packaging'), 'packaging.version': versions}
        self.modules = patch.dict(sys.modules, modules)
        self.modules.start()
        self.addCleanup(self.modules.stop)
        environment = patch.dict(os.environ, {'ARGOS_PACKAGES_DIR': str(self.root), 'XDG_CACHE_HOME': str(self.cache)})
        environment.start()
        self.addCleanup(environment.stop)
        spec = importlib.util.spec_from_file_location('tested_model_manager', SOURCE)
        self.manager = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.manager)
        self.real_download = self.manager._download_file
        self.real_sentence = self.manager._sentence_models
        self.real_catalog = self.manager._catalog
        self.manager._sentence_models = lambda codes: minisbd.download_models(codes, print)
        def download_fixture(links, destination, kind, source, target=None):
            model = next(model for model in self.catalog if model.from_code == source and model.to_code == target)
            archive = model.download()
            import shutil
            shutil.copyfile(archive, destination)
            return destination
        self.manager._download_file = download_fixture
        self.events = []
        original_progress = self.manager._progress
        def progress(*args):
            self.events.append(args)
            original_progress(*args)
        self.manager._progress = progress
        self.manager._catalog = lambda refresh, installed: self.catalog
        # Keep output quiet while retaining unittest failure details.
        self.output = contextlib.redirect_stdout(io.StringIO())
        self.output.__enter__()
        self.addCleanup(self.output.__exit__, None, None, None)

    def add(self, source, target, version='1.0', parent=None):
        metadata = {'from_code': source, 'to_code': target, 'package_version': version, 'type': 'translate'}
        path = (parent or self.root) / ('installed-' + source + '_' + target)
        (path / 'model').mkdir(parents=True)
        (path / 'metadata.json').write_text(json.dumps(metadata))
        (path / 'model/model.bin').write_bytes(b'valid')
        return path

    def available(self, source, target, version='1.0', **extra):
        model = self.Model(dict(from_code=source, to_code=target, package_version=version, **extra))
        self.catalog.append(model)
        return model

    def ready(self):
        self.add('en', 'ro')
        self.add('ro', 'en')
        self.available('en', 'ro')
        self.available('ro', 'en')

    def test_missing_pair_is_downloaded_even_when_two_models_already_exist(self):
        self.add('en', 'ro')
        stale = self.add('en', 'es')
        self.available('en', 'ro')
        self.available('ro', 'en')
        self.manager.sync_models(['en', 'ro'], prune=True)
        self.assertEqual(self.downloads, [('ro', 'en')])
        self.assertFalse(stale.exists())
        self.assertEqual(json.loads((self.base / 'model-languages.json').read_text()), ['en', 'ro'])

    def test_normal_start_does_not_upgrade_working_models(self):
        self.ready()
        self.catalog[0].package_version = '2.0'
        self.manager.sync_models(['en', 'ro'], prune=True)
        self.assertEqual(self.downloads, [])
        self.assertEqual(json.loads((self.root / 'installed-en_ro/metadata.json').read_text())['package_version'], '1.0')

    def test_failed_download_validation_preserves_deselected_models(self):
        current = self.add('en', 'ro')
        stale = self.add('en', 'es')
        self.available('en', 'ro')
        self.available('ro', 'en', damaged=True)
        with self.assertRaisesRegex(RuntimeError, 'Invalid model'):
            self.manager.sync_models(['en', 'ro'], prune=True)
        self.assertTrue(stale.exists())
        self.assertTrue(current.exists())
        self.assertFalse((self.base / 'model-languages.json').exists())

    def test_sentence_download_failure_prevents_pruning(self):
        self.ready()
        stale = self.add('en', 'es')
        self.manager._sentence_models = lambda *_: (_ for _ in ()).throw(OSError('Download failed'))
        with self.assertRaises(OSError):
            self.manager.sync_models(['en', 'ro'], prune=True)
        self.assertTrue(stale.exists())

    def test_prunes_previous_versions_and_only_private_sentence_models(self):
        self.ready()
        stale = self.add('en', 'es')
        transaction = self.base / 'model-transactions/test'
        previous = self.add('en', 'es', parent=transaction)
        previous.rename(transaction / 'old')
        (self.cache / 'minisbd/es.onnx').write_bytes(b'old sentence model')
        outside = self.base / 'outside-es.onnx'
        outside.write_bytes(b'other instance')
        self.manager.sync_models(['en', 'ro'], prune=True)
        self.assertFalse(stale.exists())
        self.assertFalse((transaction / 'old').exists())
        self.assertFalse((self.cache / 'minisbd/es.onnx').exists())
        self.assertTrue((self.cache / 'minisbd/en.onnx').exists())
        self.assertEqual(outside.read_bytes(), b'other instance')

    def test_manual_update_preserves_unselected_models_and_previous_version(self):
        self.ready()
        stale = self.add('en', 'es')
        self.catalog[0].package_version = '2.0'
        self.manager.sync_models(['en', 'ro'], update=True)
        self.assertTrue(stale.exists())
        self.assertEqual(json.loads((self.root / 'installed-en_ro/metadata.json').read_text())['package_version'], '2.0')
        backups = list((self.base / 'model-transactions').glob('*/old/metadata.json'))
        self.assertEqual(len(backups), 1)
        self.assertEqual(json.loads(backups[0].read_text())['package_version'], '1.0')

    def test_unavailable_selection_preserves_existing_models(self):
        self.ready()
        with self.assertRaisesRegex(RuntimeError, 'No models'):
            self.manager.sync_models(['en', 'es'], prune=True)
        self.assertTrue((self.root / 'installed-en_ro').exists())

    def test_updates_through_aliased_package_paths_preserve_current_models_and_one_backup(self):
        # macOS temporary paths can name the same directory through /var and
        # /private/var. An existing parent plus '..' exercises the same mismatch
        # on every platform without needing permission to create symlinks.
        (self.base / 'alias').mkdir()
        self.root = self.base / 'alias' / '..' / 'packages'
        with patch.dict(os.environ, {'ARGOS_PACKAGES_DIR': str(self.root)}):
            self.ready()
            stale = self.add('en', 'es')
            for version in ('2.0', '3.0', '4.0'):
                self.catalog[0].package_version = version
                self.manager.sync_models(['en', 'ro'], update=True)
                self.assertTrue(stale.exists())
                metadata = json.loads((self.root / 'installed-en_ro/metadata.json').read_text())
                self.assertEqual(metadata['package_version'], version)
            backups = list((self.base / 'model-transactions').glob('*/old/metadata.json'))
            self.assertEqual(len(backups), 1)
            self.assertEqual(json.loads(backups[0].read_text())['package_version'], '3.0')

    def test_refuses_paths_outside_owned_model_directory(self):
        outside = self.base / 'other-instance'
        outside.mkdir()
        with self.assertRaises(RuntimeError):
            self.manager._owned_directory(outside, self.root)
        self.assertTrue(outside.exists())

    def serve(self, payload, total):
        class Handler(BaseHTTPRequestHandler):
            def do_GET(self):
                self.send_response(200)
                if total is not None:self.send_header('Content-Length', str(total))
                self.end_headers()
                self.wfile.write(payload)
            def log_message(self, *_): pass
        server=ThreadingHTTPServer(('127.0.0.1',0),Handler)
        threading.Thread(target=server.serve_forever,daemon=True).start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        return f'http://127.0.0.1:{server.server_port}/model'

    def test_stream_reports_real_downloaded_and_total_bytes(self):
        payload=b'model bytes'*30000
        url=self.serve(payload,len(payload))
        destination=self.base/'download.argosmodel'
        self.real_download([url],destination,'translation','en','ro')
        self.assertEqual(destination.read_bytes(),payload)
        self.assertEqual(self.events[-1],('translation','en','ro','downloading',len(payload),len(payload)))
        self.assertEqual(self.events[0][-2:],(0,len(payload)))
        self.assertFalse(destination.with_name(destination.name+'.part').exists())

    def test_stream_without_content_length_keeps_total_unknown(self):
        payload=b'bytes'*200
        url=self.serve(payload,None)
        self.real_download([url],self.base/'unknown.argosmodel','sentence','en')
        self.assertEqual(self.events[-1][-2:],(len(payload),None))

    def test_truncated_download_never_commits_partial_file(self):
        url=self.serve(b'short',1024)
        destination=self.base/'truncated.argosmodel'
        with self.assertRaisesRegex(RuntimeError,'download failed'):
            self.real_download([url],destination,'translation','en','ro')
        self.assertFalse(destination.exists())
        self.assertFalse(destination.with_name(destination.name+'.part').exists())

    def test_alias_dependencies_survive_pruning_and_second_offline_start(self):
        codes = ['en', 'pb', 'zh', 'zt', 'az', 'bn', 'unknown']
        expected = {'en', 'pt', 'zh-hans', 'zh-hant', 'tr', 'hi'}
        self.manager.sbd_models.MODELS.update({code: code + '.onnx' for code in expected})
        self.assertEqual(self.manager._sentence_codes(codes), expected)
        calls = []
        def download(links, destination, kind, code, **kwargs):
            calls.append(code)
            Path(destination).write_bytes(b'valid')
        self.manager._download_file = download
        self.real_sentence(codes)
        self.assertEqual(set(calls), expected)
        transactions = self.base / 'model-transactions'
        transactions.mkdir()
        self.manager._prune(set(codes), self.root, transactions)
        self.manager._download_file = self.real_download
        self.manager._validate_sentence = lambda path: self.assertEqual(path.read_bytes(), b'valid')
        with patch.object(self.manager.urllib.request, 'urlopen', side_effect=OSError('offline')):
            self.real_sentence(codes)

    def test_invalid_sentence_model_is_replaced_only_after_validation(self):
        destination = self.cache / 'minisbd/en.onnx'
        destination.write_bytes(b'broken')
        def validate(path):
            if path.read_bytes() != b'valid':
                raise ValueError('Invalid ONNX')
        url = self.serve(b'valid', 5)
        self.real_download([url], destination, 'sentence', 'en', validate=validate)
        self.assertEqual(destination.read_bytes(), b'valid')
        destination.unlink()
        with self.assertRaisesRegex(RuntimeError, 'download failed'):
            self.real_download([self.serve(b'broken', 6)], destination, 'sentence', 'en', validate=validate)
        self.assertFalse(destination.exists())

    def test_corrupt_catalog_falls_back_to_installed_packages(self):
        self.ready()
        installed = self.manager.package.get_installed_packages()
        for data in ('{broken', '[{}]', '[]'):
            self.manager.settings.local_package_index.write_text(data)
            with patch.object(self.manager.urllib.request, 'urlopen', side_effect=OSError('offline')):
                self.assertEqual(self.real_catalog(True, installed), installed)

    def test_invalid_remote_catalog_preserves_valid_cache(self):
        original = json.dumps([dict(from_code='en', to_code='ro', package_version='1.0', links=['https://example.invalid/model'])])
        index = self.manager.settings.local_package_index
        index.write_text(original)
        with patch.object(self.manager.urllib.request, 'urlopen', return_value=io.BytesIO(b'[{}]')):
            values = self.real_catalog(True, [])
        self.assertEqual(values[0].from_code, 'en')
        self.assertEqual(index.read_text(), original)

    def test_catalog_accepts_optional_ipfs_mirrors_and_downloads_over_http(self):
        url = self.serve(b'valid', 5)
        metadata = dict(from_code='en', to_code='ro', package_version='1.0', links=['ipfs://model', url])
        with patch.object(self.manager.package, 'AvailablePackage', side_effect=lambda value: types.SimpleNamespace(**value)):
            with patch.object(self.manager.urllib.request, 'urlopen', return_value=io.BytesIO(json.dumps([metadata]).encode())):
                values = self.real_catalog(True, [])
        self.assertEqual(values[0].links, [url])
        destination = self.base / 'catalog-model.argosmodel'
        self.real_download(values[0].links, destination, 'translation', 'en', 'ro')
        self.assertEqual(destination.read_bytes(), b'valid')
        self.assertEqual(json.loads(self.manager.settings.local_package_index.read_text()), [metadata])

    def test_catalog_rejects_unsupported_or_malformed_download_links(self):
        for links in (['file:///model'], ['https://'], ['ipfs://model'], [None]):
            metadata = dict(from_code='en', to_code='ro', package_version='1.0', links=links)
            with self.assertRaisesRegex(ValueError, 'download links'):
                self.manager._parse_catalog(json.dumps([metadata]))

    def test_repeated_updates_keep_only_one_previous_version(self):
        self.ready()
        for version in ('2.0', '3.0', '4.0'):
            self.catalog[0].package_version = version
            self.manager.sync_models(['en', 'ro'], update=True)
        backups = list((self.base / 'model-transactions').glob('*/old/metadata.json'))
        self.assertEqual(len(backups), 1)
        self.assertEqual(json.loads(backups[0].read_text())['package_version'], '3.0')

    def test_download_and_extraction_limits_preserve_current_model(self):
        url = self.serve(b'123456789', None)
        destination = self.base / 'too-large.argosmodel'
        with patch.object(self.manager, 'MAX_DOWNLOAD', 5):
            with self.assertRaisesRegex(RuntimeError, 'supported size'):
                self.real_download([url], destination, 'translation', 'en', 'ro')
        self.assertFalse(destination.exists())
        self.ready()
        self.catalog[0].package_version = '2.0'
        with patch.object(self.manager, 'MAX_EXTRACTED', 1):
            with self.assertRaisesRegex(RuntimeError, 'extraction limits'):
                self.manager.sync_models(['en', 'ro'], update=True)
        self.assertEqual(json.loads((self.root / 'installed-en_ro/metadata.json').read_text())['package_version'], '1.0')

    def test_low_disk_space_stops_download_before_commit(self):
        destination = self.base / 'no-space.argosmodel'
        with patch.object(self.manager.shutil, 'disk_usage', return_value=types.SimpleNamespace(free=1)):
            with self.assertRaisesRegex(RuntimeError, 'free disk space'):
                self.real_download([self.serve(b'valid', 5)], destination, 'translation', 'en', 'ro')
        self.assertFalse(destination.exists())


if __name__ == '__main__':
    unittest.main()
