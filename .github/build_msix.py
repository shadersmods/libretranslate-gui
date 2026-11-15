"""Create an unsigned Windows 11 MSIX for testing; normal distribution requires signing."""
import argparse
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import tomllib
import xml.etree.ElementTree as ET
import zipfile

FOUNDATION = 'http://schemas.microsoft.com/appx/manifest/foundation/windows10'
UAP = 'http://schemas.microsoft.com/appx/manifest/uap/windows10'
RESCAP = 'http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities'
UNSIGNED_PUBLISHER = 'CN=shadersmods, OID.2.25.311729368913984317654407730594956997722=1'
IDENTITY = 'shadersmods.LibreTranslateGUI.Unsigned'
ET.register_namespace('', FOUNDATION)
ET.register_namespace('uap', UAP)
ET.register_namespace('rescap', RESCAP)


def element(parent, tag, **attributes):
    return ET.SubElement(parent, '{' + FOUNDATION + '}' + tag, attributes)


def manifest(version):
    if not re.fullmatch(r'(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)', version) or any(int(part) > 65535 for part in version.split('.')):
        raise ValueError('MSIX requires three numeric version parts, each at most 65535')
    package = ET.Element('{' + FOUNDATION + '}Package', IgnorableNamespaces='uap rescap')
    element(package, 'Identity', Name=IDENTITY, Publisher=UNSIGNED_PUBLISHER,
            Version=version + '.0', ProcessorArchitecture='x64')
    properties = element(package, 'Properties')
    element(properties, 'DisplayName').text = 'LibreTranslate GUI (Unsigned)'
    element(properties, 'PublisherDisplayName').text = 'shadersmods'
    element(properties, 'Description').text = 'Run and manage LibreTranslate on your own computer.'
    element(properties, 'Logo').text = 'MsixAssets/StoreLogo.png'
    resources = element(package, 'Resources')
    element(resources, 'Resource', Language='en-us')
    dependencies = element(package, 'Dependencies')
    element(dependencies, 'TargetDeviceFamily', Name='Windows.Desktop', MinVersion='10.0.22000.0', MaxVersionTested='10.0.26100.0')
    applications = element(package, 'Applications')
    application = element(applications, 'Application', Id='LibreTranslateGUI',
                          Executable='libretranslate-gui.exe', EntryPoint='Windows.FullTrustApplication')
    ET.SubElement(application, '{' + UAP + '}VisualElements', DisplayName='LibreTranslate GUI (Unsigned)',
                  Description='Run and manage LibreTranslate on your own computer.',
                  Square150x150Logo='MsixAssets/Logo150.png', Square44x44Logo='MsixAssets/Logo44.png',
                  BackgroundColor='transparent')
    capabilities = element(package, 'Capabilities')
    ET.SubElement(capabilities, '{' + RESCAP + '}Capability', Name='runFullTrust')
    ET.indent(package)
    return package


def payload_files(stage):
    required = {'libretranslate-gui.exe', 'README.md', 'THIRD_PARTY_NOTICES.md'}
    files = []
    for path in stage.rglob('*'):
        if path.is_symlink():
            raise ValueError('MSIX payload must not contain links')
        if not path.is_file():
            continue
        relative = path.relative_to(stage).as_posix()
        if relative not in required | {'locales/README.md'} and not relative.startswith(('assets/', 'docs/', 'licenses/')):
            raise ValueError('Unexpected MSIX payload: ' + relative)
        files.append(path)
    names = {path.relative_to(stage).as_posix() for path in files}
    if not required <= names or not any(name.startswith('assets/') for name in names):
        raise ValueError('MSIX requires a complete sanitized Windows release stage')
    return files


def makeappx():
    override = os.environ.get('LIBRETRANSLATE_GUI_MAKEAPPX') or shutil.which('MakeAppx.exe')
    if override:
        return Path(override)
    sdk = Path(os.environ.get('ProgramFiles(x86)', 'C:/Program Files (x86)')) / 'Windows Kits/10/bin'
    candidates = list(sdk.glob('10.*/x64/makeappx.exe'))
    if not candidates:
        raise SystemExit('Install the Windows SDK packaging tools or set LIBRETRANSLATE_GUI_MAKEAPPX.')
    return max(candidates, key=lambda path: tuple(map(int, path.parents[1].name.split('.'))))


def build(root, stage=None):
    root = Path(root).resolve()
    version = tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))['package']['version']
    stage = Path(stage or root / 'release-staging' / f'LibreTranslate-GUI-{version}-Windows-x64').resolve()
    files = payload_files(stage)
    payload = root / 'release-staging/msix/payload'
    payload.mkdir(parents=True, exist_ok=False)
    for path in files:
        destination = payload / path.relative_to(stage)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(path, destination)
    shutil.copytree(root / 'packaging/windows/msix', payload / 'MsixAssets')
    ET.ElementTree(manifest(version)).write(payload / 'AppxManifest.xml', encoding='utf-8', xml_declaration=True)
    output = root / 'release' / f'LibreTranslate-GUI-{version}-Installer-Windows-x64-Unsigned.msix'
    output.parent.mkdir(exist_ok=True)
    subprocess.run([str(makeappx()), 'pack', '/d', str(payload), '/p', str(output), '/o'], check=True)
    verify(output, stage)
    digest = hashlib.sha256(output.read_bytes()).hexdigest()
    output.with_suffix('.msix.sha256').write_text(f'{digest}  {output.name}\n', encoding='ascii')
    return output


def verify(package, stage):
    stage = Path(stage)
    with zipfile.ZipFile(package) as archive:
        if archive.testzip() is not None:
            raise ValueError('MSIX contains a corrupted file')
        for path in payload_files(stage):
            relative = path.relative_to(stage).as_posix()
            if archive.read(relative) != path.read_bytes():
                raise ValueError('MSIX payload differs: ' + relative)
        document = ET.fromstring(archive.read('AppxManifest.xml'))
        identity = document.find('{' + FOUNDATION + '}Identity')
        if identity.get('Publisher') != UNSIGNED_PUBLISHER or identity.get('Name') != IDENTITY:
            raise ValueError('Unexpected unsigned MSIX identity')
        expected = {path.relative_to(stage).as_posix() for path in payload_files(stage)}
        expected |= {'AppxManifest.xml', 'AppxBlockMap.xml', '[Content_Types].xml',
                     'MsixAssets/Logo44.png', 'MsixAssets/Logo150.png', 'MsixAssets/StoreLogo.png'}
        if set(archive.namelist()) != expected:
            raise ValueError('Unexpected files in unsigned MSIX payload')
    print('Unsigned MSIX payload verified:', Path(package).name)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--stage', type=Path)
    args = parser.parse_args()
    print(build(Path(__file__).resolve().parents[1], args.stage))
