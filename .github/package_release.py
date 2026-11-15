"""Package only program files, never a local instance or preferences."""
import hashlib
import os
import platform
import shutil
import tarfile
import tomllib
import zipfile
from pathlib import Path

root = Path(__file__).resolve().parents[1]
version = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
tag = os.environ.get("GITHUB_REF_NAME", "v" + version)
if tag != "v" + version:
    raise SystemExit("Release tag must match Cargo.toml")
system = {"Windows": "Windows", "Darwin": "macOS", "Linux": "Linux"}[platform.system()]
arch = {"AMD64": "x64", "x86_64": "x64", "arm64": "arm64", "aarch64": "arm64"}[platform.machine()]
executable = "libretranslate-gui.exe" if system == "Windows" else "libretranslate-gui"
target = Path(os.environ.get("CARGO_TARGET_DIR", str(root / "target")))
candidates = [p for p in (target / "dx").rglob(executable)
              if "release" in p.parts and (p.parent / "assets").is_dir()]
if len(candidates) != 1:
    raise SystemExit("Expected one Dioxus release executable with its assets folder")
name = f"LibreTranslate-GUI-{version}-{system}-{arch}"
staging = root / "release-staging" / name
staging.mkdir(parents=True, exist_ok=False)
shutil.copy2(candidates[0], staging / executable)
shutil.copytree(candidates[0].parent / "assets", staging / "assets")
shutil.copytree(root / "licenses", staging / "licenses")
for filename in ("README.md", "THIRD_PARTY_NOTICES.md"):
    shutil.copy2(root / filename, staging / filename)
(staging / "locales").mkdir()
shutil.copy2(root / "locales" / "README.md", staging / "locales" / "README.md")
output = root / "release"
output.mkdir(exist_ok=True)
if system == "Linux":
    archive = output / (name + ".tar.gz")
    with tarfile.open(archive, "w:gz") as bundle:
        bundle.add(staging, arcname=name)
else:
    archive = output / (name + ".zip")
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as bundle:
        for path in staging.rglob("*"):
            if path.is_file():
                bundle.write(path, path.relative_to(staging.parent))
digest = hashlib.sha256(archive.read_bytes()).hexdigest()
archive.with_suffix(archive.suffix + ".sha256").write_text(f"{digest}  {archive.name}\n", encoding="ascii")
print(archive.name)
