"""Package only program files, never a local instance or preferences."""
import argparse
import hashlib
import os
import platform
import shutil
import tarfile
import tomllib
import zipfile
from pathlib import Path


def package(root, *, system=None, machine=None, target=None, tag=None):
    root = Path(root)
    version = tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))['package']['version']
    tag = tag if tag is not None else os.environ.get('GITHUB_REF_NAME') or 'v' + version
    if tag != 'v' + version:
        raise SystemExit('Release tag must match Cargo.toml')
    system = {'Windows': 'Windows', 'Darwin': 'macOS', 'Linux': 'Linux'}[system or platform.system()]
    arch = {'AMD64': 'x64', 'x86_64': 'x64', 'arm64': 'arm64', 'aarch64': 'arm64'}[machine or platform.machine()]
    executable = 'libretranslate-gui.exe' if system == 'Windows' else 'libretranslate-gui'
    target = Path(target or os.environ.get('CARGO_TARGET_DIR', str(root / 'target')))
    if system == 'macOS':
        candidates = [p for p in (target / 'dx').rglob('*.app') if 'release' in p.parts
                      and (p / 'Contents/MacOS' / executable).is_file()
                      and (p / 'Contents/Resources/assets').is_dir()
                      and (p / 'Contents/Info.plist').is_file()]
    else:
        candidates = [p for p in (target / 'dx').rglob(executable)
                      if 'release' in p.parts and (p.parent / 'assets').is_dir()]
    if len(candidates) != 1:
        raise SystemExit('Expected one complete Dioxus release app bundle' if system == 'macOS'
                         else 'Expected one Dioxus release executable with its assets folder')
    name = f'LibreTranslate-GUI-{version}-{system}-{arch}'
    staging = root / 'release-staging' / name
    staging.mkdir(parents=True, exist_ok=False)
    mac_executable = None
    if system == 'macOS':
        source = candidates[0]
        contents = staging / source.name / 'Contents'
        (contents / 'MacOS').mkdir(parents=True)
        mac_executable = contents / 'MacOS' / executable
        shutil.copy2(source / 'Contents/MacOS' / executable, mac_executable)
        # Preserve native bundle files without including data/settings created
        # beside the executable by a previous local launch.
        for entry in (source / 'Contents').iterdir():
            if entry.name in {'Resources', 'Frameworks', 'PlugIns', 'SharedSupport', '_CodeSignature'}:
                shutil.copytree(entry, contents / entry.name, symlinks=True)
            elif entry.name in {'Info.plist', 'PkgInfo'}:
                shutil.copy2(entry, contents / entry.name)
    else:
        shutil.copy2(candidates[0], staging / executable)
        shutil.copytree(candidates[0].parent / 'assets', staging / 'assets')
    shutil.copytree(root / 'licenses', staging / 'licenses')
    shutil.copytree(root / 'docs', staging / 'docs')
    for filename in ('README.md', 'THIRD_PARTY_NOTICES.md'):
        shutil.copy2(root / filename, staging / filename)
    (staging / 'locales').mkdir()
    shutil.copy2(root / 'locales/README.md', staging / 'locales/README.md')
    output = root / 'release'
    output.mkdir(exist_ok=True)
    if system == 'Linux':
        archive = output / (name + '.tar.gz')
        with tarfile.open(archive, 'w:gz') as bundle:
            bundle.add(staging, arcname=name)
    else:
        archive = output / (name + '.zip')
        with zipfile.ZipFile(archive, 'w', zipfile.ZIP_DEFLATED) as bundle:
            for path in staging.rglob('*'):
                relative = path.relative_to(staging.parent).as_posix()
                if path.is_symlink():
                    info = zipfile.ZipInfo(relative)
                    info.create_system = 3
                    info.external_attr = 0o120777 << 16
                    bundle.writestr(info, os.readlink(path))
                elif path.is_file():
                    info = zipfile.ZipInfo.from_file(path, relative)
                    info.compress_type = zipfile.ZIP_DEFLATED
                    if path == mac_executable:
                        info.create_system = 3
                        info.external_attr = 0o100755 << 16
                    with path.open('rb') as source, bundle.open(info, 'w') as destination:
                        shutil.copyfileobj(source, destination)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_suffix(archive.suffix + '.sha256').write_text(f'{digest}  {archive.name}\n', encoding='ascii')
    return archive


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--current-version', action='store_true',
                        help='Package a branch build using Cargo.toml, without checking the GitHub ref')
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    tag = None
    if args.current_version:
        tag = 'v' + tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))['package']['version']
    print(package(root, tag=tag).name)
