"""Build a per-user Windows installer from the sanitized portable release stage."""
import argparse
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import tomllib
import uuid
import xml.etree.ElementTree as ET

NS = 'http://wixtoolset.org/schemas/v4/wxs'
ET.register_namespace('', NS)
NAMESPACE = uuid.UUID('c32c5fb5-d24e-424e-b350-9d54dc5a075f')
MARKER = 'libretranslate-gui.install.json'
MARKER_CONTENT = '{"schema":1,"kind":"msi"}\n'
NAME = 'LibreTranslate GUI'
REGISTRY = r'Software\shadersmods\LibreTranslate GUI'


def node(parent, tag, **attributes):
    return ET.SubElement(parent, '{' + NS + '}' + tag, attributes)


def identifier(prefix, value):
    return prefix + hashlib.sha256(value.casefold().encode('utf-8')).hexdigest()[:32]


def guid(value):
    return str(uuid.uuid5(NAMESPACE, value.casefold())).upper()


def validate_version(version):
    if not re.fullmatch(r'(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)', version):
        raise ValueError('MSI releases require a three-part numeric version')
    if any(int(part) > limit for part, limit in zip(version.split('.'), (255, 255, 65535))):
        raise ValueError('Version exceeds Windows Installer limits')


def generate(stage, output, version, icon, *, identity='release', product_name=NAME):
    """identity is only overridden by isolated install/upgrade tests."""
    validate_version(version)
    stage, output = Path(stage).resolve(), Path(output).resolve()
    files = sorted(path for path in stage.rglob('*') if path.is_file())
    required = {'libretranslate-gui.exe', 'README.md', 'THIRD_PARTY_NOTICES.md'}
    names = set()
    for path in stage.rglob('*'):
        relative = path.relative_to(stage).as_posix()
        if path.is_symlink():
            raise ValueError('Installer payload must not contain links')
        if not path.is_file():
            continue
        if relative not in required | {'locales/README.md'} and not relative.startswith(('assets/', 'docs/', 'licenses/')):
            raise ValueError('Unexpected installer payload: ' + relative)
        if relative.casefold() in names:
            raise ValueError('Case-colliding installer payload: ' + relative)
        names.add(relative.casefold())
    if not {name.casefold() for name in required} <= names or not any(name.startswith('assets/') for name in names):
        raise ValueError('Installer requires a complete Windows release stage')
    output.parent.mkdir(parents=True, exist_ok=True)
    marker = output.parent / MARKER
    marker.write_text(MARKER_CONTENT, encoding='utf-8')
    payload = [(path, path.relative_to(stage).as_posix()) for path in files]
    payload.append((marker, MARKER))
    payload.append((Path(__file__).resolve().parents[1] / 'packaging/windows/SetupUI.wxs',
                    'licenses/wix/SetupUI.wxs'))
    tree = ET.Element('{' + NS + '}Wix')
    package = node(tree, 'Package', Name=product_name, Manufacturer='shadersmods',
                   Version=version, Language='1033', Scope='perUser',
                   ProductCode=guid(identity + ':x64:product:' + version),
                   UpgradeCode=guid(identity + ':x64:upgrade'), InstallerVersion='500')
    node(package, 'MajorUpgrade', Schedule='afterInstallInitialize',
         DowngradeErrorMessage='A newer version of [ProductName] is already installed.')
    node(package, 'MediaTemplate', EmbedCab='yes', CompressionLevel='high')
    node(package, 'Icon', Id='AppIcon', SourceFile=str(Path(icon).resolve()))
    node(package, 'Property', Id='ARPPRODUCTICON', Value='AppIcon')
    node(package, 'Property', Id='ARPHELPLINK', Value='https://github.com/shadersmods/libretranslate-gui/issues')
    node(package, 'Property', Id='ARPURLINFOABOUT', Value='https://github.com/shadersmods/libretranslate-gui')
    registry = REGISTRY if identity == 'release' else REGISTRY + '\\Tests\\' + identifier('t', identity)
    remembered = node(package, 'Property', Id='INSTALLFOLDER')
    node(remembered, 'RegistrySearch', Id='PreviousInstallFolder', Root='HKCU', Key=registry,
         Name='InstallDir', Type='raw')
    local = node(package, 'StandardDirectory', Id='LocalAppDataFolder')
    programs = node(local, 'Directory', Id='UserPrograms', Name='Programs')
    install = node(programs, 'Directory', Id='INSTALLFOLDER', Name=product_name)
    directories = {'': install}

    def directory(relative):
        if relative not in directories:
            parent, _, name = relative.rpartition('/')
            directories[relative] = node(directory(parent), 'Directory', Id=identifier('dir', relative), Name=name)
        return directories[relative]

    feature = node(package, 'Feature', Id='Application', Title=product_name, Level='1')
    for path, relative in payload:
        parent, _, filename = relative.rpartition('/')
        component_id = identifier('cmp', relative)
        component = node(directory(parent), 'Component', Id=component_id,
                         Guid=guid(identity + ':x64:file:' + relative))
        file = node(component, 'File', Id=identifier('file', relative), Name=filename,
                    Source=str(path), KeyPath='no')
        node(component, 'RegistryValue', Root='HKCU', Key=registry + '\\Components',
             Name=component_id, Value='1', Type='integer', KeyPath='yes')
        node(feature, 'ComponentRef', Id=component_id)
        if relative == 'libretranslate-gui.exe':
            node(file, 'Shortcut', Id='StartMenuShortcut', Directory='ProgramMenuFolder',
                 Name=product_name, WorkingDirectory='INSTALLFOLDER', Advertise='no', Icon='AppIcon')
            node(component, 'RegistryValue', Root='HKCU', Key=registry, Name='InstallDir',
                 Value='[INSTALLFOLDER]', Type='string')
    # Remove only empty program directories. User-created files are never MSI components.
    for relative, folder in list(directories.items()):
        component_id = identifier('cleanup', relative)
        component = node(folder, 'Component', Id=component_id,
                         Guid=guid(identity + ':x64:directory:' + relative))
        node(component, 'RegistryValue', Root='HKCU', Key=registry + '\\Components',
             Name=component_id, Value='1', Type='integer', KeyPath='yes')
        node(component, 'RemoveFolder', Id=identifier('remove', relative), On='uninstall')
        if not relative:
            node(component, 'RemoveFolder', Id='RemoveEmptyPrograms', Directory='UserPrograms', On='uninstall')
        node(feature, 'ComponentRef', Id=component_id)
    node(package, 'StandardDirectory', Id='ProgramMenuFolder')
    node(package, 'UIRef', Id='LibreTranslateSetup')
    ET.indent(tree)
    ET.ElementTree(tree).write(output, encoding='utf-8', xml_declaration=True)
    return tree


def build(root, stage=None, *, version=None, identity='release', product_name=NAME, work=None):
    root = Path(root).resolve()
    version = version or tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))['package']['version']
    legacy_name = f'LibreTranslate-GUI-{version}-Windows-x64'
    name = f'LibreTranslate-GUI-{version}-Installer-Windows-x64'
    stage = Path(stage or root / 'release-staging' / legacy_name).resolve()
    work = Path(work or root / 'release-staging/wix').resolve()
    work.mkdir(parents=True, exist_ok=True)
    source = work / 'Installer.wxs'
    generate(stage, source, version, root / 'assets/icon.ico', identity=identity, product_name=product_name)
    msbuild = shutil.which('msbuild') or os.environ.get('LIBRETRANSLATE_GUI_MSBUILD')
    if not msbuild:
        raise SystemExit('MSBuild is required. Run in a Visual Studio developer shell or use microsoft/setup-msbuild in CI.')
    output = root / 'release' if identity == 'release' else work / 'output'
    output.mkdir(parents=True, exist_ok=True)
    subprocess.run([msbuild, str(root / 'packaging/windows/LibreTranslateGui.wixproj'),
                    '/restore', '/verbosity:minimal', '/p:Configuration=Release',
                    '/p:InstallerPlatform=x64', '/p:InstallerSource=' + str(source),
                    '/p:InstallerName=' + name, '/p:OutputPath=' + str(output) + os.sep,
                    '/p:IntermediateOutputPath=' + str(work / 'obj') + os.sep,
                    '/p:MSBuildProjectExtensionsPath=' + str(work / 'restore') + os.sep], check=True)
    installer = output / (name + '.msi')
    digest = hashlib.sha256(installer.read_bytes()).hexdigest()
    installer.with_suffix('.msi.sha256').write_text(f'{digest}  {installer.name}\n', encoding='ascii')
    if identity == 'release':
        legacy = output / (legacy_name + '.msi')
        shutil.copy2(installer, legacy)
        legacy.with_suffix('.msi.sha256').write_text(f'{digest}  {legacy.name}\n', encoding='ascii')
    return installer


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--stage', type=Path)
    args = parser.parse_args()
    print(build(Path(__file__).resolve().parents[1], args.stage))
