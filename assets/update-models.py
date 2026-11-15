"""Install selected Argos pairs before retiring unselected models."""
import gc
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import time
import urllib.request
from urllib.parse import urlsplit
import uuid
import zipfile

import ctranslate2
from argostranslate import package, settings, translate, sbd
from libretranslate.language import iso2model
import minisbd.models as sbd_models
from packaging.version import Version

_transfer_sizes = {}
MAX_DOWNLOAD = 8 * 1024**3
MAX_SENTENCE_DOWNLOAD = 256 * 1024**2
MAX_EXTRACTED = 16 * 1024**3
MAX_ARCHIVE_FILES = 50000
MAX_CATALOG = 16 * 1024**2
DISK_RESERVE = 128 * 1024**2


def _space(path, required):
    if shutil.disk_usage(path).free < required + DISK_RESERVE:
        raise RuntimeError('Not enough free disk space for the model. Free space and retry.')


def _progress(kind, source, target, phase, downloaded, total):
    _transfer_sizes[(kind, source, target)] = (downloaded, total)
    print('@ltgui-model:' + json.dumps(dict(kind=kind, source=source, target=target,
          phase=phase, downloaded=downloaded, total=total)), flush=True)


def _transfer_failed(kind, source, target):
    downloaded, total = _transfer_sizes.get((kind, source, target), (0, None))
    _progress(kind, source, target, 'failed', downloaded, total)


def _download_file(links, destination, kind, source, target=None, validate=None):
    destination = Path(destination)
    if destination.is_file():
        try:
            if validate:
                validate(destination)
            return destination
        except Exception:
            destination.unlink()
    destination.parent.mkdir(parents=True, exist_ok=True)
    partial = destination.with_name(destination.name + '.part')
    last_error = None
    limit = MAX_SENTENCE_DOWNLOAD if kind == 'sentence' else MAX_DOWNLOAD
    for link in links:
        for _ in range(2):
            downloaded, total = 0, None
            try:
                request = urllib.request.Request(link, headers={'User-Agent': 'LibreTranslate-GUI'})
                with urllib.request.urlopen(request, timeout=30) as response, partial.open('wb') as output:
                    header = response.headers.get('Content-Length', '')
                    total = int(header) if header.isdecimal() and int(header) > 0 else None
                    if total is not None and total > limit:
                        raise RuntimeError('The model download exceeds the supported size.')
                    _space(destination.parent, total or 0)
                    _progress(kind, source, target, 'downloading', 0, total)
                    last = time.monotonic()
                    while chunk := response.read(128 * 1024):
                        downloaded += len(chunk)
                        if downloaded > limit:
                            raise RuntimeError('The model download exceeds the supported size.')
                        if total is not None and downloaded > total:
                            raise RuntimeError('The model download exceeded its reported size.')
                        if total is None:
                            _space(destination.parent, len(chunk))
                        output.write(chunk)
                        if time.monotonic() - last >= 0.2:
                            _progress(kind, source, target, 'downloading', downloaded, total)
                            last = time.monotonic()
                    _progress(kind, source, target, 'downloading', downloaded, total)
                    if downloaded == 0 or (total is not None and downloaded != total):
                        raise RuntimeError('The model download is incomplete.')
                    output.flush()
                    os.fsync(output.fileno())
                if validate:
                    validate(partial)
                os.replace(partial, destination)
                return destination
            except Exception as error:
                last_error = error
                partial.unlink(missing_ok=True)
    raise RuntimeError(f'Model download failed: {last_error or "No download links are available."}')


def _sentence_codes(selected):
    mapping = sbd.MiniSBDSentencizer.LANGUAGE_CODE_MAPPING
    codes = {mapping.get(code, code) for code in selected}
    return {code if code in sbd_models.MODELS else 'en' for code in codes}


def _validate_sentence(path):
    import onnxruntime
    options = onnxruntime.SessionOptions()
    options.intra_op_num_threads = 1
    options.inter_op_num_threads = 1
    session = onnxruntime.InferenceSession(str(path), sess_options=options, providers=['CPUExecutionProvider'])
    del session


def _sentence_models(selected):
    for code in sorted(_sentence_codes(selected)):
        filename = sbd_models.MODELS[code]
        path = Path(sbd_models.cache_dir) / filename
        try:
            _progress('sentence', code, None, 'queued', 0, None)
            _download_file([sbd_models.REPO_URL + filename], path, 'sentence', code, validate=_validate_sentence)
            size = path.stat().st_size
            _progress('sentence', code, None, 'complete', size, size)
        except Exception:
            _transfer_failed('sentence', code, None)
            raise


def _owned_directory(path, parent):
    path, parent = Path(path), Path(parent).resolve()
    if path.parent.resolve() != parent or path.resolve().parent != parent or path.is_symlink():
        raise RuntimeError('The model directory is outside this instance.')
    return path


def _parse_catalog(data):
    values = json.loads(data)
    if not isinstance(values, list) or not values:
        raise ValueError('The model catalog is empty or invalid.')
    packages = []
    for value in values:
        if not isinstance(value, dict) or not all(isinstance(value.get(key), str) and value[key]
                for key in ('from_code', 'to_code', 'package_version')):
            raise ValueError('The model catalog contains an invalid package.')
        Version(value['package_version'])
        if not isinstance(value.get('links'), list) or not value['links'] or not all(
                isinstance(link, str) and urlsplit(link).scheme in ('https', 'http', 'ipfs')
                and urlsplit(link).netloc for link in value['links']):
            raise ValueError('The model catalog contains invalid download links.')
        # The official index includes optional IPFS mirrors. urllib cannot open
        # those, so retain the supported mirrors without rejecting the package.
        links = [link for link in value['links'] if urlsplit(link).scheme in ('https', 'http')]
        if not links:
            raise ValueError('The model catalog contains no supported download links.')
        packages.append(package.AvailablePackage(dict(value, links=links)))
    return packages


def _catalog(refresh, installed):
    index = Path(settings.local_package_index)
    if refresh or not index.is_file():
        temporary = index.with_name('index-' + str(uuid.uuid4()) + '.tmp')
        try:
            with urllib.request.urlopen(settings.remote_package_index, timeout=10) as response:
                data = response.read(MAX_CATALOG + 1)
            if len(data) > MAX_CATALOG:
                raise ValueError('The model catalog is too large.')
            _parse_catalog(data)
            index.parent.mkdir(parents=True, exist_ok=True)
            temporary.write_bytes(data)
            os.replace(temporary, index)
        except Exception as error:
            print(f'Could not refresh the model catalog; using local models: {error}', flush=True)
        finally:
            temporary.unlink(missing_ok=True)
    if index.is_file():
        try:
            if index.stat().st_size > MAX_CATALOG:
                raise ValueError('The model catalog is too large.')
            return _parse_catalog(index.read_bytes())
        except Exception as error:
            print(f'Could not read the model catalog; using installed models: {error}', flush=True)
    # Existing installations can still start offline when their catalog is missing.
    return installed


def _install(candidate, current, installed, root, transactions):
    source, target = candidate.from_code, candidate.to_code
    try:
        archive = Path(settings.downloads_dir) / (package.argospm_package_name(candidate) + '.argosmodel')
        _download_file(candidate.links, archive, 'translation', source, target)
        size = archive.stat().st_size
        _progress('translation', source, target, 'validating', size, size)
        try:
            _install_archive(candidate, current, installed, root, transactions, archive)
        except (zipfile.BadZipFile, RuntimeError):
            archive.unlink(missing_ok=True)
            raise
        _progress('translation', source, target, 'complete', size, size)
        archive.unlink(missing_ok=True)
    except Exception:
        _transfer_failed('translation', source, target)
        raise


def _install_archive(candidate, current, installed, root, transactions, archive):
    pair = f'{candidate.from_code} → {candidate.to_code}'
    print(f'Downloading {pair} ({candidate.package_version})', flush=True)
    transaction = transactions / str(uuid.uuid4())
    transaction.mkdir()
    staging = transaction / 'staging'
    staging.mkdir()
    with zipfile.ZipFile(archive) as source:
        members = source.infolist()
        size = sum(member.file_size for member in members)
        if len(members) > MAX_ARCHIVE_FILES or size > MAX_EXTRACTED:
            raise RuntimeError('The model archive exceeds the extraction limits.')
        _space(staging, size)
        for member in members:
            relative = PurePosixPath(member.filename.replace('\\', '/'))
            if relative.is_absolute() or '..' in relative.parts or any(':' in part for part in relative.parts):
                raise RuntimeError('The model archive contains an invalid path.')
        if source.testzip() is not None:
            source.close()
            if archive.parent == settings.downloads_dir:
                archive.unlink(missing_ok=True)
            raise RuntimeError('The model archive is damaged. Retry to download it again.')
        source.extractall(staging)
    ready = None
    for metadata in staging.glob('*/metadata.json'):
        value = json.loads(metadata.read_text(encoding='utf-8'))
        if value.get('from_code') == candidate.from_code and value.get('to_code') == candidate.to_code and value.get('package_version') == candidate.package_version:
            ready = metadata.parent
            break
    if ready is None:
        raise RuntimeError('The downloaded model does not match the selected language pair.')
    check = ctranslate2.Translator(str(ready / 'model'), device='cpu', inter_threads=1, intra_threads=1)
    del check
    gc.collect()
    destination = _owned_directory(root / (current.package_path.name if current else ready.name), root)
    with open(transaction / 'destination.json', 'w', encoding='utf-8') as journal:
        json.dump(destination.name, journal)
        journal.flush()
        os.fsync(journal.fileno())
    backup = transaction / 'old'
    if destination.exists():
        destination.rename(backup)
    try:
        ready.rename(destination)
    except BaseException:
        if backup.exists() and not destination.exists():
            backup.rename(destination)
        raise
    print(f'Installed {pair} ({candidate.package_version})', flush=True)
    for old in installed:
        if old.package_path != destination and old.package_path.exists():
            _owned_directory(old.package_path, root).rename(transaction / ('previous-' + old.package_path.name))


def _unselected(metadata, selected):
    return metadata.get('type', 'translate') == 'translate' and (
        metadata.get('from_code') not in selected or metadata.get('to_code') not in selected
    )


def _prune(selected, root, transactions):
    # Retire old versions first so interrupted-update recovery cannot restore them later.
    for transaction in transactions.iterdir():
        if not transaction.is_dir():
            continue
        _owned_directory(transaction, transactions)
        for previous in transaction.iterdir():
            if previous.name != 'old' and not previous.name.startswith('previous-'):
                continue
            if not previous.is_dir():
                continue
            _owned_directory(previous, transaction)
            metadata = previous / 'metadata.json'
            if metadata.is_file() and _unselected(json.loads(metadata.read_text(encoding='utf-8')), selected):
                shutil.rmtree(previous)
    for model in package.get_installed_packages():
        if model.type == 'translate' and (model.from_code not in selected or model.to_code not in selected):
            path = _owned_directory(model.package_path, root)
            shutil.rmtree(path)
            print(f'Removed model: {model.from_code} → {model.to_code}', flush=True)
    # MiniSBD caches belong to the same instance, never the user's system cache.
    cache = Path(sbd_models.cache_dir)
    expected = Path(os.environ['XDG_CACHE_HOME']) / 'minisbd'
    if cache.resolve() != expected.resolve():
        raise RuntimeError('The sentence model cache is outside this instance.')
    needed = {sbd_models.MODELS[code] for code in _sentence_codes(selected)}
    caches = {cache}
    legacy = Path(settings.data_dir) / 'minisbd'
    if legacy.resolve().is_relative_to(root.parent):
        caches.add(legacy)
    for private_cache in caches:
        for filename in set(sbd_models.MODELS.values()) - needed:
            path = private_cache / filename
            if path.is_file():
                if path.resolve().parent != private_cache.resolve():
                    raise RuntimeError('The sentence model is outside this instance.')
                path.unlink()


def _retention(root, transactions):
    # Keep one previous version per pair. Never discard an unfinished recovery.
    backups = {}
    disposable = []
    for transaction in transactions.iterdir():
        if not transaction.is_dir():
            continue
        _owned_directory(transaction, transactions)
        journal = transaction / 'destination.json'
        if journal.is_file():
            target = _owned_directory(root / json.loads(journal.read_text(encoding='utf-8')), root)
            if not target.exists():
                continue
        disposable.append(transaction)
        for previous in transaction.iterdir():
            if not previous.is_dir() or (previous.name != 'old' and not previous.name.startswith('previous-')):
                continue
            _owned_directory(previous, transaction)
            metadata = previous / 'metadata.json'
            if not metadata.is_file():
                continue
            value = json.loads(metadata.read_text(encoding='utf-8'))
            pair = value['from_code'], value['to_code']
            backups.setdefault(pair, []).append((Version(value['package_version']), previous.stat().st_mtime_ns, previous))
    keep = {max(items, key=lambda item: (item[0], item[1]))[2] for items in backups.values()}
    for transaction in disposable:
        for child in transaction.iterdir():
            if child.is_dir() and child not in keep:
                shutil.rmtree(_owned_directory(child, transaction))
        if not any(child in keep for child in transaction.iterdir()):
            shutil.rmtree(_owned_directory(transaction, transactions))


def sync_models(codes, update=False, prune=False):
    selected = set(iso2model(codes))
    root = Path(os.environ['ARGOS_PACKAGES_DIR']).resolve()
    transactions = root.parent / 'model-transactions'
    transactions.mkdir(exist_ok=True)
    _owned_directory(transactions, root.parent)
    marker = root.parent / 'model-languages.json'
    try:
        previous = set(json.loads(marker.read_text(encoding='utf-8')))
    except (OSError, ValueError):
        previous = set()
    installed = package.get_installed_packages()
    print('Checking selected language models', flush=True)
    available = _catalog(update or selected != previous, installed)
    pairs = {}
    for candidate in available:
        if candidate.type != 'translate' or candidate.from_code not in selected or candidate.to_code not in selected:
            continue
        pair = (candidate.from_code, candidate.to_code)
        if pair not in pairs or Version(candidate.package_version) > Version(pairs[pair].package_version):
            pairs[pair] = candidate
    covered = {code for pair in pairs for code in pair}
    if not pairs or selected - covered:
        raise RuntimeError('No models are available for all selected languages. Add English for wider model coverage.')
    required = []
    for pair, candidate in sorted(pairs.items()):
        matching = [model for model in installed if (model.from_code, model.to_code) == pair]
        current = max(matching, key=lambda model: Version(model.package_version), default=None)
        present = current and (current.package_path / 'model' / 'model.bin').is_file()
        if present and (not update or Version(current.package_version) >= Version(candidate.package_version)):
            continue
        required.append((candidate, current, matching))
        _progress('translation', *pair, 'queued', 0, None)
    for candidate, current, matching in required:
        _install(candidate, current, matching, root, transactions)
    _sentence_models(selected)
    # Deletions happen only after every required download and validation succeeds.
    if prune:
        _prune(selected, root, transactions)
    try:
        _retention(root, transactions)
    except Exception as error:
        print(f'Storage cleanup deferred: {error}', flush=True)
    translate.get_installed_languages.cache_clear()
    temporary = marker.with_suffix('.json.tmp')
    temporary.write_text(json.dumps(sorted(selected)), encoding='utf-8')
    os.replace(temporary, marker)
    print('Model update complete.' if update else 'Selected language models are ready.', flush=True)
