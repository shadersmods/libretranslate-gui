//! Verified updates use Windows Installer for MSI installations and file transactions for archives.
use crate::app_updates::{REPOSITORY, Release};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const MAX_DOWNLOAD: u64 = 256 * 1024 * 1024;
const MAX_EXPANDED: u64 = 768 * 1024 * 1024;
#[path = "update_storage.rs"]
mod storage;
#[derive(Clone, Copy, Debug, Default)]
pub struct Progress {
    pub downloaded: u64,
    pub total: u64,
    pub phase: u8,
}
pub type SharedProgress = Arc<Mutex<Progress>>;
#[derive(Clone, Serialize, Deserialize)]
pub struct Job {
    root: PathBuf,
    staging: PathBuf,
    executable: String,
    parent: u32,
    version: String,
    files: Vec<String>,
    #[serde(default)]
    msi: bool,
}

pub fn platform() -> Option<&'static str> {
    if crate::installation::is_msix() {
        return Some(if crate::installation::is_unsigned_msix() {
            "Windows-x64-Unsigned.msix"
        } else {
            "Windows-x64.msix"
        });
    }
    platform_package(
        std::env::consts::OS,
        std::env::consts::ARCH,
        crate::installation::is_msi(),
    )
}
fn platform_package(os: &str, arch: &str, msi: bool) -> Option<&'static str> {
    match (os, arch) {
        ("windows", "x86_64") if msi => Some("Windows-x64.msi"),
        ("windows", "x86_64") => Some("Windows-x64.zip"),
        ("macos", "x86_64") => Some("macOS-x64.zip"),
        ("macos", "aarch64") => Some("macOS-arm64.zip"),
        ("linux", "x86_64") => Some("Linux-x64.tar.gz"),
        _ => None,
    }
}
pub fn package_name(release: &Release) -> Option<String> {
    let platform = platform()?;
    Some(format!(
        "LibreTranslate-GUI-{}-{}-{}",
        release.tag_name.trim_start_matches('v'),
        if platform.ends_with(".msi") || platform.ends_with(".msix") {
            "Installer"
        } else {
            "Portable"
        },
        platform
    ))
}
pub fn legacy_package_name(release: &Release) -> Option<String> {
    Some(format!(
        "LibreTranslate-GUI-{}-{}",
        release.tag_name.trim_start_matches('v'),
        platform()?
    ))
}
fn official_asset(url: &str) -> bool {
    url.starts_with(&format!("{REPOSITORY}/releases/download/"))
}
fn fetch_url(original: &str) -> Result<String> {
    #[cfg(feature = "ui-test")]
    if let Ok(endpoint) = std::env::var("LIBRETRANSLATE_GUI_TEST_UPDATE_ARCHIVE") {
        let url = reqwest::Url::parse(&endpoint)?;
        ensure!(
            url.scheme() == "http" && url.host_str() == Some("127.0.0.1"),
            "Invalid UI-test download endpoint"
        );
        return Ok(endpoint);
    }
    Ok(original.to_string())
}
fn download_client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(180))
        .connect_timeout(Duration::from_secs(15))
        .user_agent(concat!("LibreTranslate-GUI/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() > 5 {
                return attempt.error("Too many redirects");
            }
            let url = attempt.url();
            if url.scheme() == "https"
                && matches!(
                    url.host_str(),
                    Some(
                        "github.com"
                            | "release-assets.githubusercontent.com"
                            | "objects.githubusercontent.com"
                    )
                )
            {
                attempt.follow()
            } else {
                attempt.error("Untrusted update redirect")
            }
        }))
        .build()?)
}
fn checksum(
    release: &Release,
    asset: &crate::app_updates::Asset,
    client: &reqwest::blocking::Client,
) -> Result<String> {
    if let Some(value) = asset
        .digest
        .as_deref()
        .and_then(|value| value.strip_prefix("sha256:"))
    {
        return valid_hash(value);
    }
    let name = format!("{}.sha256", asset.name);
    let companion = release
        .assets
        .iter()
        .find(|candidate| candidate.name == name && official_asset(&candidate.browser_download_url))
        .context("The release has no SHA-256 checksum")?;
    let response = client
        .get(fetch_url(&companion.browser_download_url)?)
        .send()?
        .error_for_status()?;
    let mut bytes = Vec::new();
    response.take(4097).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 4096, "Checksum file is too large");
    let text = std::str::from_utf8(&bytes)?;
    let mut parts = text.split_whitespace();
    let hash = valid_hash(parts.next().unwrap_or_default())?;
    ensure!(
        parts.next().map(|name| name.trim_start_matches('*')) == Some(asset.name.as_str()),
        "Checksum does not name the release archive"
    );
    Ok(hash)
}
fn valid_hash(value: &str) -> Result<String> {
    ensure!(
        value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "Invalid SHA-256 checksum"
    );
    Ok(value.to_ascii_lowercase())
}
fn create_parent(root: &Path, relative: &Path) -> Result<()> {
    let mut current = root.to_path_buf();
    for part in relative.components() {
        let Component::Normal(part) = part else {
            bail!("Invalid program directory")
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => ensure!(
                metadata.is_dir()
                    && !metadata.file_type().is_symlink()
                    && storage::same_path(&dunce::canonicalize(&current)?, &current),
                "Update destination contains a link or non-directory"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => fs::create_dir(&current)?,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

pub fn prepare(release: &Release, progress: SharedProgress) -> Result<Job> {
    ensure!(
        !crate::installation::is_msix(),
        "Windows manages MSIX installations. Download and install the MSIX update instead."
    );
    ensure!(
        crate::app_updates::newer(&release.tag_name, env!("CARGO_PKG_VERSION")),
        "The release is not newer than this app"
    );
    let asset = release
        .update_asset()
        .context("No compatible update package for this installation")?;
    ensure!(
        asset.size > 0 && asset.size <= MAX_DOWNLOAD,
        "Invalid update size"
    );
    let current = dunce::canonicalize(std::env::current_exe()?)?;
    let root = application_root(&current)?.to_path_buf();
    let mut preparation =
        storage::Preparation::new(&root).context("The application folder is not writable")?;
    let staging = preparation.path().to_path_buf();
    let client = download_client()?;
    let expected = checksum(release, asset, &client)?;
    *progress.lock().unwrap() = Progress {
        total: asset.size,
        ..Default::default()
    };
    let mut response = client
        .get(fetch_url(&asset.browser_download_url)?)
        .send()?
        .error_for_status()?;
    let msi = crate::installation::is_msi();
    let archive = staging.join(if msi {
        "release.msi"
    } else {
        "release.archive"
    });
    let mut file = fs::File::create(&archive)?;
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut bytes = [0; 64 * 1024];
    loop {
        let size = response.read(&mut bytes)?;
        if size == 0 {
            break;
        }
        count += size as u64;
        ensure!(
            count <= asset.size && count <= MAX_DOWNLOAD,
            "Update is larger than advertised"
        );
        file.write_all(&bytes[..size])?;
        hash.update(&bytes[..size]);
        progress.lock().unwrap().downloaded = count;
    }
    file.sync_all()?;
    ensure!(count == asset.size, "Incomplete update download");
    progress.lock().unwrap().phase = 1;
    ensure!(
        format!("{:x}", hash.finalize()) == expected,
        "Update checksum mismatch"
    );
    let files = if msi {
        Vec::new()
    } else {
        let unpacked = staging.join("bundle");
        fs::create_dir(&unpacked)?;
        let package = asset.name.replace("-Portable-", "-");
        let prefix = package.trim_end_matches(".tar.gz").trim_end_matches(".zip");
        extract(&archive, &unpacked, prefix, asset.name.ends_with(".zip"))?
    };
    progress.lock().unwrap().phase = 2;
    let executable = current
        .strip_prefix(&root)?
        .to_str()
        .context("Invalid executable name")?
        .replace('\\', "/");
    let job = Job {
        root,
        staging,
        executable,
        parent: std::process::id(),
        version: release.tag_name.clone(),
        files,
        msi,
    };
    let helper = job.staging.join(if cfg!(windows) {
        "helper.exe"
    } else {
        "helper"
    });
    fs::copy(current, helper)?;
    fs::write(job.staging.join("job.json"), serde_json::to_vec(&job)?)?;
    preparation.ready()?;
    Ok(job)
}
pub fn cleanup_old_stages() -> Result<()> {
    let executable = dunce::canonicalize(std::env::current_exe()?)?;
    storage::maintain(application_root(&executable)?)
}
fn application_root(executable: &Path) -> Result<&Path> {
    let parent = executable.parent().context("No application folder")?;
    let bundle = parent.ancestors().nth(2);
    if parent.file_name().is_some_and(|name| name == "MacOS")
        && parent
            .parent()
            .is_some_and(|path| path.ends_with("Contents"))
        && let Some(bundle) = bundle.filter(|path| path.extension().is_some_and(|ext| ext == "app"))
    {
        return bundle.parent().context("No application bundle folder");
    }
    Ok(parent)
}
fn macos_bundle_path(path: &Path) -> Option<&Path> {
    let mut parts = path.components();
    let Component::Normal(bundle) = parts.next()? else {
        return None;
    };
    if Path::new(bundle).extension()? != "app"
        || parts.next()? != Component::Normal("Contents".as_ref())
    {
        return None;
    }
    Some(parts.as_path())
}
fn is_executable(path: &Path) -> bool {
    path == Path::new(executable_name())
        || macos_bundle_path(path)
            .is_some_and(|inner| inner == Path::new("MacOS/libretranslate-gui"))
}
fn executable_name() -> &'static str {
    if cfg!(windows) {
        "libretranslate-gui.exe"
    } else {
        "libretranslate-gui"
    }
}
fn allowed(path: &Path) -> bool {
    if path
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
    {
        return false;
    }
    let text = path.to_string_lossy();
    if text.contains(['\\', ':']) {
        return false;
    }
    is_executable(path)
        || matches!(
            text.as_ref(),
            "README.md" | "THIRD_PARTY_NOTICES.md" | "locales/README.md"
        )
        || text.starts_with("assets/")
        || text.starts_with("licenses/")
        || text.starts_with("docs/")
        || macos_bundle_path(path).is_some_and(|inner| {
            matches!(inner.to_str(), Some("Info.plist" | "PkgInfo"))
                || [
                    "Resources",
                    "Frameworks",
                    "PlugIns",
                    "SharedSupport",
                    "_CodeSignature",
                ]
                .iter()
                .any(|directory| inner.starts_with(directory))
        })
}
fn relative_entry(name: &Path, prefix: &str) -> Result<PathBuf> {
    let mut parts = name.components();
    ensure!(
        matches!(parts.next(),Some(Component::Normal(name))if name==prefix),
        "Unexpected package root"
    );
    let path = parts.as_path().to_path_buf();
    ensure!(
        allowed(&path),
        "Unexpected file in update bundle: {}",
        name.display()
    );
    Ok(path)
}
fn extract(
    archive: &Path,
    destination: &Path,
    prefix: &str,
    zip_archive: bool,
) -> Result<Vec<String>> {
    let mut files = Vec::new();
    let mut expanded = 0u64;
    let mut write_entry = |name: &Path, size: u64, reader: &mut dyn Read| -> Result<()> {
        ensure!(files.len() < 10000, "Too many update files");
        expanded = expanded.checked_add(size).context("Update size overflow")?;
        ensure!(
            expanded <= MAX_EXPANDED,
            "Update expands beyond the size limit"
        );
        let relative = relative_entry(name, prefix)?;
        let text = relative
            .to_str()
            .context("Invalid update filename")?
            .replace('\\', "/");
        ensure!(!files.contains(&text), "Duplicate update file");
        let target = destination.join(&relative);
        fs::create_dir_all(target.parent().unwrap())?;
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)?;
        let copied = std::io::copy(&mut reader.take(size + 1), &mut output)?;
        ensure!(copied == size, "Incorrect expanded file size");
        output.sync_all()?;
        files.push(text);
        Ok(())
    };
    if zip_archive {
        let mut archive = zip::ZipArchive::new(fs::File::open(archive)?)?;
        ensure!(archive.len() <= 10000, "Too many update entries");
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            ensure!(
                entry
                    .unix_mode()
                    .is_none_or(|mode| mode & 0o170000 != 0o120000),
                "Symlink in update"
            );
            if entry.is_dir() {
                continue;
            }
            let name = PathBuf::from(
                entry
                    .name()
                    .context("Invalid update ZIP filename")?
                    .as_ref(),
            );
            let size = entry.size();
            write_entry(&name, size, &mut entry)?;
        }
    } else {
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(archive)?));
        for entry in archive.entries()? {
            let mut entry = entry?;
            let kind = entry.header().entry_type();
            if kind.is_dir() {
                continue;
            }
            ensure!(kind.is_file(), "Link or special file in update");
            let name = entry.path()?.into_owned();
            let size = entry.size();
            write_entry(&name, size, &mut entry)?;
        }
    }
    let executables = files
        .iter()
        .filter(|name| is_executable(Path::new(name)))
        .collect::<Vec<_>>();
    ensure!(
        executables.len() == 1,
        "Update is missing its executable or contains multiple apps"
    );
    let executable = Path::new(executables[0]);
    if macos_bundle_path(executable).is_some() {
        ensure!(
            files
                .iter()
                .all(|name| macos_bundle_path(Path::new(name)).is_none()
                    || Path::new(name).components().next() == executable.components().next()),
            "Update contains multiple application bundles"
        );
    }
    let assets = if macos_bundle_path(executable).is_some() {
        PathBuf::from(executable.components().next().unwrap().as_os_str())
            .join("Contents/Resources/assets")
    } else {
        PathBuf::from("assets")
    };
    ensure!(
        files
            .iter()
            .any(|name| Path::new(name).starts_with(&assets) && Path::new(name) != assets),
        "Update is missing its assets"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            destination.join(executable),
            fs::Permissions::from_mode(0o755),
        )?;
    }
    Ok(files)
}
pub fn launch_helper(job: &Job) -> Result<()> {
    launch_helper_mode(job, "--apply-update")
}
fn launch_helper_mode(job: &Job, mode: &str) -> Result<()> {
    let mut command = std::process::Command::new(job.staging.join(if cfg!(windows) {
        "helper.exe"
    } else {
        "helper"
    }));
    command
        .arg(mode)
        .arg(job.staging.join("job.json"))
        .current_dir(&job.root);
    let log_path = job.staging.join("helper.log");
    if log_path.exists() {
        ensure!(
            !fs::symlink_metadata(&log_path)?.file_type().is_symlink(),
            "Invalid update helper log"
        );
    }
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    command.stdout(log.try_clone()?).stderr(log);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    if let Err(error) = command.spawn() {
        if mode == "--apply-update" {
            let _ = storage::discard_ready(job);
        }
        return Err(error.into());
    }
    Ok(())
}
fn validate_job(job: &Job) -> Result<()> {
    let root = dunce::canonicalize(&job.root)?;
    let staging = dunce::canonicalize(&job.staging)?;
    ensure!(
        storage::same_path(&root, &job.root)
            && storage::same_path(&staging, &job.staging)
            && staging
                .parent()
                .is_some_and(|parent| dunce::canonicalize(root.join(".updates"))
                    .is_ok_and(|expected| storage::same_path(parent, &expected))),
        "Invalid update paths"
    );
    ensure!(
        (Path::new(&job.executable).components().count() == 1
            && job.executable != "."
            && job.executable != ".."
            && !job.executable.contains(['/', '\\', ':']))
            || (allowed(Path::new(&job.executable)) && is_executable(Path::new(&job.executable))),
        "Invalid executable name"
    );
    ensure!(
        job.files.len() <= 10000 && job.files.iter().all(|file| allowed(Path::new(file))),
        "Invalid program file list"
    );
    if job.msi {
        ensure!(
            cfg!(windows)
                && crate::installation::is_msi_at(&root)
                && job.files.is_empty()
                && job.executable == "libretranslate-gui.exe",
            "Invalid Windows Installer update job"
        );
    }
    ensure!(
        semver::Version::parse(job.version.trim_start_matches('v'))
            .is_ok_and(|version| version.pre.is_empty()),
        "Invalid version"
    );
    Ok(())
}
fn target(job: &Job, relative: &str) -> PathBuf {
    if let Some(inner) = macos_bundle_path(Path::new(relative))
        && macos_bundle_path(Path::new(&job.executable)).is_some()
        && let Some(Component::Normal(bundle)) = Path::new(&job.executable).components().next()
    {
        return job.root.join(bundle).join("Contents").join(inner);
    }
    job.root.join(if relative == executable_name() {
        job.executable.as_str()
    } else {
        relative
    })
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    complete: bool,
    changes: Vec<Change>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    relative: String,
    original: bool,
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    fs::File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
fn durable_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("Missing journal folder")?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    sync_directory(parent)
}
fn read_journal(stage: &Path) -> Result<Option<Journal>> {
    let path = stage.join("transaction.json");
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    ensure!(
        metadata.is_file()
            && !metadata.file_type().is_symlink()
            && metadata.len() <= 2 * 1024 * 1024,
        "Invalid update journal"
    );
    Ok(Some(serde_json::from_slice(&fs::read(path)?)?))
}
fn rollback(job: &Job) -> Result<()> {
    validate_job(job)?;
    let Some(journal) = read_journal(&job.staging)? else {
        return Ok(());
    };
    if journal.complete {
        return Ok(());
    }
    if let Some(newer) = superseding_update(job)? {
        durable_json(
            &job.staging.join("superseded-by.json"),
            &serde_json::json!({"stage":newer.staging,"version":newer.version}),
        )?;
        fs::remove_file(job.staging.join("transaction.json"))?;
        sync_directory(&job.staging)?;
        storage::finished(job, false)?;
        return Ok(());
    }
    ensure!(
        journal.changes.len() <= job.files.len(),
        "Invalid recovery file count"
    );
    for change in journal.changes.iter().rev() {
        ensure!(
            job.files.contains(&change.relative),
            "Invalid recovery file"
        );
        let destination = target(job, &change.relative);
        create_parent(
            &job.root,
            destination.strip_prefix(&job.root)?.parent().unwrap(),
        )?;
        if let Ok(metadata) = fs::symlink_metadata(&destination) {
            ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "Invalid recovery destination"
            );
        }
        if change.original {
            let original = job.staging.join("backup").join(&change.relative);
            let metadata = fs::symlink_metadata(&original)?;
            ensure!(
                metadata.is_file()
                    && !metadata.file_type().is_symlink()
                    && dunce::canonicalize(&original)?
                        .starts_with(dunce::canonicalize(job.staging.join("backup"))?),
                "Invalid recovery backup"
            );
            // Copy, rather than consume, the backup so another interruption is recoverable.
            let mut restored = tempfile::NamedTempFile::new_in(destination.parent().unwrap())?;
            std::io::copy(&mut fs::File::open(&original)?, restored.as_file_mut())?;
            restored.as_file().set_permissions(metadata.permissions())?;
            restored.as_file().sync_all()?;
            restored.persist(&destination)?;
        } else if destination.exists() {
            fs::remove_file(&destination)?;
        }
        sync_directory(destination.parent().unwrap())?;
    }
    fs::remove_file(job.staging.join("transaction.json"))?;
    sync_directory(&job.staging)
}

fn stage_job(root: &Path, stage: &Path) -> Result<Job> {
    let path = stage.join("job.json");
    let metadata = fs::symlink_metadata(&path)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() < 128 * 1024,
        "Invalid recovery job"
    );
    let job: Job = serde_json::from_slice(&fs::read(path)?)?;
    validate_job(&job)?;
    ensure!(
        storage::same_path(&job.root, root) && storage::same_path(&job.staging, stage),
        "Invalid recovery location"
    );
    Ok(job)
}

// A legacy helper can install a newer release while an earlier failed attempt
// still has a journal. That earlier backup must never undo the successful install.
fn superseding_update(job: &Job) -> Result<Option<Job>> {
    let Some(created) = storage::created(&job.staging) else {
        return Ok(None);
    };
    let version = semver::Version::parse(job.version.trim_start_matches('v'))?;
    for entry in fs::read_dir(job.root.join(".updates"))? {
        let entry = entry?;
        if !entry.file_type()?.is_dir()
            || entry.file_type()?.is_symlink()
            || storage::same_path(&entry.path(), &job.staging)
        {
            continue;
        }
        if storage::created(&entry.path()).is_none_or(|time| time < created) {
            continue;
        }
        if !read_journal(&entry.path())
            .ok()
            .flatten()
            .is_some_and(|journal| journal.complete)
        {
            continue;
        }
        let Ok(candidate) = stage_job(&job.root, &entry.path()) else {
            continue;
        };
        if semver::Version::parse(candidate.version.trim_start_matches('v'))? >= version {
            return Ok(Some(candidate));
        }
    }
    Ok(None)
}

fn recover_other_transactions(job: &Job) -> Result<()> {
    let mut pending = Vec::new();
    for entry in fs::read_dir(job.root.join(".updates"))? {
        let entry = entry?;
        if !entry.file_type()?.is_dir()
            || entry.file_type()?.is_symlink()
            || storage::same_path(&entry.path(), &job.staging)
        {
            continue;
        }
        if read_journal(&entry.path())?.is_none_or(|journal| journal.complete) {
            continue;
        }
        let lease = storage::lease(&entry.path())?
            .context("Another update is still running. Close other app windows and try again.")?;
        let previous = stage_job(&job.root, &entry.path())?;
        ensure!(
            !parent_alive(previous.parent),
            "Another app window is using an interrupted update. Close it before updating."
        );
        pending.push((
            storage::created(&entry.path()).unwrap_or_default(),
            previous,
            lease,
        ));
    }
    pending.sort_by_key(|(created, _, _)| std::cmp::Reverse(*created));
    for (_, previous, _lease) in pending {
        rollback(&previous)
            .context("Could not recover the previous update before installing this one")?;
        storage::finished(&previous, false)?;
    }
    Ok(())
}

/// Run recovery before loading any bundle assets; the helper can replace the executable on Windows.
pub fn recover_if_needed() -> Result<bool> {
    let executable = dunce::canonicalize(std::env::current_exe()?)?;
    let root = application_root(&executable)?;
    let updates = root.join(".updates");
    if !updates.is_dir() {
        return Ok(false);
    }
    let _coordination = match coordination_lock(root) {
        Ok(lock) => lock,
        Err(_) => return Ok(false),
    };
    for entry in fs::read_dir(&updates)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() || entry.file_type()?.is_symlink() {
            continue;
        }
        if read_journal(&entry.path())?.is_none_or(|journal| journal.complete) {
            continue;
        }
        let Some(lease) = storage::lease(&entry.path())? else {
            continue;
        };
        let mut job: Job = serde_json::from_slice(&fs::read(entry.path().join("job.json"))?)?;
        validate_job(&job)?;
        ensure!(
            storage::same_path(&job.root, root) && storage::same_path(&job.staging, &entry.path()),
            "Invalid recovery location"
        );
        if parent_alive(job.parent) {
            continue;
        }
        if superseding_update(&job)?.is_some() {
            rollback(&job)?;
            continue;
        }
        // Recovery must use the current recovery rules, including when the
        // failed transaction was created by an older release's helper.
        let helper = job.staging.join(if cfg!(windows) {
            "helper.exe"
        } else {
            "helper"
        });
        let copied = tempfile::NamedTempFile::new_in(&job.staging)?;
        fs::copy(&executable, copied.path())?;
        copied.as_file().sync_all()?;
        copied.persist(helper)?;
        job.parent = std::process::id();
        durable_json(&job.staging.join("job.json"), &job)?;
        drop(lease);
        launch_helper_mode(&job, "--recover-update")?;
        return Ok(true);
    }
    Ok(false)
}

fn install(job: &Job) -> Result<()> {
    validate_job(job)?;
    recover_other_transactions(job)?;
    if job.msi {
        return install_msi(job);
    }
    rollback(job)?;
    let bundle = job.staging.join("bundle");
    let backup = job.staging.join("backup");
    let mut journal = Journal::default();
    durable_json(&job.staging.join("transaction.json"), &journal)?;
    let mut files = job.files.clone();
    files.sort_by_key(|name| is_executable(Path::new(name)));
    let result = (|| -> Result<()> {
        for relative in &files {
            let destination = target(job, relative);
            let original = backup.join(relative);
            let mut existed = false;
            create_parent(
                &job.root,
                destination.strip_prefix(&job.root)?.parent().unwrap(),
            )?;
            // Reject an existing symlink or a destination directory; never replace data trees.
            if let Ok(metadata) = fs::symlink_metadata(&destination) {
                ensure!(
                    metadata.is_file() && !metadata.file_type().is_symlink(),
                    "Program destination is not a regular file"
                );
                fs::create_dir_all(original.parent().unwrap())?;
                // Keep the current app launchable until the staged file replaces
                // it atomically; moving it away first leaves a crash-time gap.
                fs::copy(&destination, &original)?;
                fs::OpenOptions::new()
                    .write(true)
                    .open(&original)?
                    .sync_all()?;
                sync_directory(original.parent().unwrap())?;
                existed = true;
            }
            journal.changes.push(Change {
                relative: relative.clone(),
                original: existed,
            });
            durable_json(&job.staging.join("transaction.json"), &journal)?;
            fs::rename(bundle.join(relative), &destination)?;
            sync_directory(destination.parent().unwrap())?;
        }
        journal.complete = true;
        durable_json(&job.staging.join("transaction.json"), &journal)?;
        Ok(())
    })();
    if result.is_err() {
        rollback(job).context("Could not restore the previous program files")?;
    }
    result
}
fn install_msi(job: &Job) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::{ffi::OsStrExt, process::CommandExt};

        let package = job.staging.join("release.msi");
        ensure!(
            storage::same_path(&dunce::canonicalize(&package)?, &package)
                && fs::metadata(&package)?.is_file(),
            "Invalid staged installer"
        );
        let directory = job.root.as_os_str();
        ensure!(
            !directory.encode_wide().any(|unit| unit == u16::from(b'"')),
            "Invalid installer directory"
        );
        // MSI parses PROPERTY="value", while arg() would quote the entire
        // INSTALLFOLDER=value token when the path contains spaces.
        let mut install_folder = std::ffi::OsString::from("INSTALLFOLDER=\"");
        install_folder.push(directory);
        install_folder.push("\"");
        let windows = std::env::var_os("SystemRoot").context("Windows directory is unavailable")?;
        let status =
            std::process::Command::new(PathBuf::from(windows).join("System32/msiexec.exe"))
                .arg("/i")
                .arg(package)
                .args(["/passive", "/norestart", "REBOOT=ReallySuppress"])
                .raw_arg(install_folder)
                .arg("/L*v")
                .arg(job.staging.join("installer.log"))
                .status()?;
        ensure!(
            matches!(status.code(), Some(0 | 3010)),
            "Windows Installer returned {}. See {} for details.",
            status,
            job.staging.join("installer.log").display()
        );
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = job;
        bail!("Windows Installer updates are only available on Windows")
    }
}
fn parent_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    if pid > i32::MAX as u32 {
        return false;
    }
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, WAIT_TIMEOUT},
            System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
        };
        let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return false;
        }
        let alive = WaitForSingleObject(handle, 0) == WAIT_TIMEOUT;
        CloseHandle(handle);
        alive
    }
    #[cfg(unix)]
    unsafe {
        libc::kill(pid as i32, 0) == 0
    }
}
fn coordination_lock(root: &Path) -> Result<fs::File> {
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join(".updates/helper.lock"))?;
    fs2::FileExt::try_lock_exclusive(&lock).context("Another update helper is already running")?;
    Ok(lock)
}
pub fn run_helper_if_requested() -> bool {
    let mut args = std::env::args_os();
    let _ = args.next();
    let mode = args.next();
    let recovery = mode.as_deref() == Some(std::ffi::OsStr::new("--recover-update"));
    if !recovery && mode.as_deref() != Some(std::ffi::OsStr::new("--apply-update")) {
        return false;
    }
    let result = (|| -> Result<()> {
        let file = PathBuf::from(args.next().context("Missing update job")?);
        ensure!(
            fs::metadata(&file)?.len() < 128 * 1024,
            "Update job is too large"
        );
        let job: Job = serde_json::from_slice(&fs::read(&file)?)?;
        validate_job(&job)?;
        let helper = dunce::canonicalize(std::env::current_exe()?)?;
        ensure!(
            helper.parent() == Some(job.staging.as_path()) && file == job.staging.join("job.json"),
            "Update helper must run from its staging folder"
        );
        let _update_lock = coordination_lock(&job.root)?;
        let stage_lock =
            storage::lease(&job.staging)?.context("This update stage is already active")?;
        // Helper mode returns directly from main. Keep this lease until process
        // teardown so the restarted app cannot reclaim an executable still mapped
        // by the exiting helper.
        std::mem::forget(stage_lock);
        let started = Instant::now();
        while parent_alive(job.parent) {
            ensure!(
                started.elapsed() < Duration::from_secs(120),
                "The app did not close in time"
            );
            std::thread::sleep(Duration::from_millis(100));
        }
        let result = if recovery {
            rollback(&job)
        } else {
            install(&job)
        };
        if let Err(error) = storage::finished(&job, result.is_ok() && !recovery) {
            eprintln!("Could not record update cleanup state: {error:#}");
        }
        fs::write(
            job.root.join(".updates/last-result.json"),
            serde_json::to_vec(
                &serde_json::json!({"ok":result.is_ok(),"error":result.as_ref().err().map(|error|format!("{error:#}"))}),
            )?,
        )?;
        // A failed transaction restores old program files before reopening them.
        std::process::Command::new(job.root.join(&job.executable))
            .current_dir(&job.root)
            .spawn()?;
        result
    })();
    if let Err(error) = result {
        eprintln!("Update failed: {error:#}");
    }
    true
}
pub fn previous_failure() -> Option<String> {
    let executable = std::env::current_exe().ok()?;
    let path = application_root(&executable)
        .ok()?
        .join(".updates/last-result.json");
    if fs::metadata(&path).ok()?.len() > 16384 {
        return None;
    }
    let data: serde_json::Value = serde_json::from_slice(&fs::read(&path).ok()?).ok()?;
    let _ = fs::remove_file(path);
    if data["ok"] == true {
        None
    } else {
        Some(
            data["error"]
                .as_str()
                .unwrap_or("Update failed")
                .to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    #[ignore = "requires the disposable MSI products created by tests/windows_installer.py --smoke"]
    fn msi_update_installs_into_paths_with_spaces() {
        let job_file = std::env::var_os("LIBRETRANSLATE_GUI_TEST_MSI_JOB")
            .map(PathBuf::from)
            .expect("Missing disposable installer job");
        let job: Job = serde_json::from_slice(&fs::read(&job_file).unwrap()).unwrap();
        let fixtures =
            dunce::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join(".runtime-test"))
                .unwrap();
        assert!(
            dunce::canonicalize(&job.root)
                .unwrap()
                .starts_with(&fixtures)
        );
        assert_eq!(job_file, job.staging.join("job.json"));
        assert!(job.root.as_os_str().to_string_lossy().contains(' '));
        assert!(job.msi);
        install(&job).unwrap();
        assert!(job.staging.join("installer.log").is_file());
    }

    fn tracked_job(root: &Path, created: u64, version: &str) -> Job {
        let mut job = job(root, vec![executable_name()]);
        let stage = job
            .root
            .join(".updates")
            .join(uuid::Uuid::new_v4().to_string());
        fs::rename(&job.staging, &stage).unwrap();
        job.staging = stage;
        job.version = version.into();
        fs::write(
            job.staging.join("job.json"),
            serde_json::to_vec(&job).unwrap(),
        )
        .unwrap();
        fs::write(
            job.staging.join("stage.json"),
            serde_json::to_vec(
                &serde_json::json!({"schema":1,"created":created,"finished":null,"phase":"ready"}),
            )
            .unwrap(),
        )
        .unwrap();
        job
    }
    fn interrupted(job: &Job, backup: &str) {
        fs::create_dir_all(job.staging.join("backup")).unwrap();
        fs::write(job.staging.join("backup").join(executable_name()), backup).unwrap();
        durable_json(
            &job.staging.join("transaction.json"),
            &Journal {
                complete: false,
                changes: vec![Change {
                    relative: executable_name().into(),
                    original: true,
                }],
            },
        )
        .unwrap();
    }
    #[test]
    fn legacy_interrupted_update_cannot_undo_a_later_successful_update() {
        let dir = tempfile::tempdir().unwrap();
        let earlier = tracked_job(dir.path(), 10, "v0.3.0");
        interrupted(&earlier, "old 0.2.7");
        let applied = tracked_job(dir.path(), 20, "v0.3.0");
        durable_json(
            &applied.staging.join("transaction.json"),
            &Journal {
                complete: true,
                changes: vec![],
            },
        )
        .unwrap();
        fs::write(dir.path().join(executable_name()), "new 0.3.0").unwrap();
        fs::write(
            dir.path().join("libretranslate-desktop.json"),
            "private settings",
        )
        .unwrap();
        rollback(&earlier).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join(executable_name())).unwrap(),
            "new 0.3.0"
        );
        assert!(!earlier.staging.join("transaction.json").exists());
        assert!(earlier.staging.join("superseded-by.json").is_file());
        assert_eq!(
            fs::read_to_string(dir.path().join("libretranslate-desktop.json")).unwrap(),
            "private settings"
        );
    }
    #[test]
    fn new_update_recovers_previous_partial_files_before_taking_its_backup() {
        let dir = tempfile::tempdir().unwrap();
        let earlier = tracked_job(dir.path(), 10, "v0.3.0");
        interrupted(&earlier, "last working binary");
        fs::write(
            dir.path().join(executable_name()),
            "partial earlier attempt",
        )
        .unwrap();
        let next = tracked_job(dir.path(), 20, "v0.3.0");
        fs::write(
            next.staging.join("bundle").join(executable_name()),
            "new binary",
        )
        .unwrap();
        install(&next).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join(executable_name())).unwrap(),
            "new binary"
        );
        assert_eq!(
            fs::read_to_string(next.staging.join("backup").join(executable_name())).unwrap(),
            "last working binary"
        );
        assert!(!earlier.staging.join("transaction.json").exists());
    }
    #[test]
    fn failed_attempt_after_a_successful_install_still_restores_that_install() {
        let dir = tempfile::tempdir().unwrap();
        let applied = tracked_job(dir.path(), 10, "v0.3.0");
        durable_json(
            &applied.staging.join("transaction.json"),
            &Journal {
                complete: true,
                changes: vec![],
            },
        )
        .unwrap();
        let failed = tracked_job(dir.path(), 20, "v0.4.0");
        interrupted(&failed, "working 0.3.0");
        fs::write(dir.path().join(executable_name()), "partial 0.4.0").unwrap();
        rollback(&failed).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join(executable_name())).unwrap(),
            "working 0.3.0"
        );
    }
    #[test]
    fn invalid_parent_pids_never_refer_to_a_process_group() {
        assert!(!parent_alive(0));
        #[cfg(unix)]
        assert!(!parent_alive(u32::MAX));
    }
    #[cfg(windows)]
    #[test]
    fn update_paths_allow_windows_case_variants_without_allowing_links() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".Updates")).unwrap();
        let job = job(dir.path(), vec![executable_name()]);
        validate_job(&job).unwrap();
        create_parent(&job.root, Path::new(".updates/nested")).unwrap();
        assert!(dir.path().join(".Updates/nested").is_dir());
    }
    fn job(root: &Path, files: Vec<&str>) -> Job {
        let root = dunce::canonicalize(root).unwrap();
        let staging = root.join(".updates/test");
        fs::create_dir_all(staging.join("bundle")).unwrap();
        Job {
            root,
            staging,
            executable: executable_name().into(),
            parent: 0,
            version: "v0.3.0".into(),
            files: files.into_iter().map(str::to_string).collect(),
            msi: false,
        }
    }
    #[test]
    fn updates_match_the_installed_distribution_and_old_jobs_stay_portable() {
        assert_eq!(
            platform_package("windows", "x86_64", true),
            Some("Windows-x64.msi")
        );
        assert_eq!(
            platform_package("windows", "x86_64", false),
            Some("Windows-x64.zip")
        );
        assert_eq!(
            platform_package("macos", "aarch64", false),
            Some("macOS-arm64.zip")
        );
        let dir = tempfile::tempdir().unwrap();
        let original = job(dir.path(), vec![executable_name()]);
        let mut json = serde_json::to_value(&original).unwrap();
        json.as_object_mut().unwrap().remove("msi");
        assert!(!serde_json::from_value::<Job>(json).unwrap().msi);
        let mut invalid = original;
        invalid.msi = true;
        assert!(validate_job(&invalid).is_err());
        #[cfg(windows)]
        {
            fs::write(
                invalid.root.join("libretranslate-gui.install.json"),
                br#"{"schema":1,"kind":"msi"}"#,
            )
            .unwrap();
            assert!(
                validate_job(&invalid).is_err(),
                "MSI jobs cannot replace loose program files"
            );
            invalid.files.clear();
            validate_job(&invalid).unwrap();
        }
    }
    #[test]
    fn rejects_escape_paths_and_data_files() {
        for name in [
            "pkg/../data/instances.json",
            "other/assets/test.svg",
            "pkg/data/instances.json",
            "pkg/libretranslate-desktop.json",
            "pkg/assets/../../secrets",
            "pkg/assets/C:evil",
            "pkg/Example.app/Contents/MacOS/data/instances.json",
            "pkg/Example.app/Contents/MacOS/libretranslate-desktop.json",
            "pkg/Example.app/Contents/../private",
        ] {
            assert!(relative_entry(Path::new(name), "pkg").is_err(), "{name}");
        }
        assert!(relative_entry(Path::new("pkg/assets/flag.svg"), "pkg").is_ok());
    }
    #[test]
    fn native_macos_bundle_updates_and_rollbacks_preserve_data_and_renamed_apps() {
        for fail in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let mut job = job(dir.path(), vec![]);
            job.executable = "Renamed.app/Contents/MacOS/libretranslate-gui".into();
            let archive = dir.path().join("release.zip");
            let mut writer = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
            for name in [
                "Release.app/Contents/Info.plist",
                "Release.app/Contents/Resources/assets/new.css",
                "Release.app/Contents/MacOS/libretranslate-gui",
                "docs/usage.md",
            ] {
                writer
                    .start_file(
                        format!("bundle/{name}"),
                        zip::write::SimpleFileOptions::default(),
                    )
                    .unwrap();
                writer.write_all(b"new program file").unwrap();
            }
            writer.finish().unwrap();
            job.files = extract(&archive, &job.staging.join("bundle"), "bundle", true).unwrap();
            let executable = job.root.join(&job.executable);
            assert_eq!(application_root(&executable).unwrap(), job.root);
            fs::create_dir_all(executable.parent().unwrap().join("data")).unwrap();
            fs::write(&executable, "old executable").unwrap();
            let data = executable.parent().unwrap().join("data/instances.json");
            let preferences = executable
                .parent()
                .unwrap()
                .join("libretranslate-desktop.json");
            fs::write(&data, "private instances").unwrap();
            fs::write(&preferences, "private preferences").unwrap();
            let plist = job.root.join("Renamed.app/Contents/Info.plist");
            fs::write(&plist, "old metadata").unwrap();
            if fail {
                fs::remove_file(
                    job.staging
                        .join("bundle/Release.app/Contents/MacOS/libretranslate-gui"),
                )
                .unwrap();
                assert!(install(&job).is_err());
                assert_eq!(fs::read_to_string(&executable).unwrap(), "old executable");
                assert_eq!(fs::read_to_string(plist).unwrap(), "old metadata");
                assert!(!job.root.join("docs/usage.md").exists());
            } else {
                install(&job).unwrap();
                assert_eq!(fs::read_to_string(&executable).unwrap(), "new program file");
                assert_eq!(fs::read_to_string(plist).unwrap(), "new program file");
                assert_eq!(
                    fs::read_to_string(job.root.join("docs/usage.md")).unwrap(),
                    "new program file"
                );
            }
            assert_eq!(fs::read_to_string(data).unwrap(), "private instances");
            assert_eq!(
                fs::read_to_string(preferences).unwrap(),
                "private preferences"
            );
            assert!(!job.root.join("Release.app").exists());
        }
    }
    #[test]
    fn replaces_program_files_and_keeps_data_preferences_and_unknown_assets() {
        let dir = tempfile::tempdir().unwrap();
        let job = job(dir.path(), vec![executable_name(), "assets/new.svg"]);
        fs::create_dir_all(job.root.join("assets")).unwrap();
        fs::create_dir_all(job.root.join("data")).unwrap();
        for (name, value) in [
            (executable_name(), "old"),
            ("data/instances.json", "private"),
            ("libretranslate-desktop.json", "preferences"),
            ("assets/custom.txt", "custom"),
        ] {
            fs::write(job.root.join(name), value).unwrap();
        }
        fs::create_dir_all(job.staging.join("bundle/assets")).unwrap();
        fs::write(job.staging.join("bundle").join(executable_name()), "new").unwrap();
        fs::write(job.staging.join("bundle/assets/new.svg"), "svg").unwrap();
        install(&job).unwrap();
        assert_eq!(
            fs::read_to_string(job.root.join(executable_name())).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(job.root.join("data/instances.json")).unwrap(),
            "private"
        );
        assert_eq!(
            fs::read_to_string(job.root.join("libretranslate-desktop.json")).unwrap(),
            "preferences"
        );
        assert_eq!(
            fs::read_to_string(job.root.join("assets/custom.txt")).unwrap(),
            "custom"
        );
        assert_eq!(
            fs::read_to_string(job.staging.join("backup").join(executable_name())).unwrap(),
            "old"
        );
    }
    #[test]
    fn failed_install_restores_every_changed_file() {
        let dir = tempfile::tempdir().unwrap();
        let job = job(dir.path(), vec!["assets/a.svg", executable_name()]);
        fs::create_dir_all(job.root.join("assets")).unwrap();
        fs::create_dir_all(job.staging.join("bundle/assets")).unwrap();
        fs::write(job.root.join("assets/a.svg"), "old asset").unwrap();
        fs::write(job.root.join(executable_name()), "old binary").unwrap();
        fs::write(job.staging.join("bundle/assets/a.svg"), "new asset").unwrap();
        assert!(install(&job).is_err());
        assert_eq!(
            fs::read_to_string(job.root.join("assets/a.svg")).unwrap(),
            "old asset"
        );
        assert_eq!(
            fs::read_to_string(job.root.join(executable_name())).unwrap(),
            "old binary"
        );
    }
    #[test]
    fn journal_recovers_each_interrupted_replacement_and_is_repeatable() {
        for interrupted_after in 0..=3 {
            let dir = tempfile::tempdir().unwrap();
            let job = job(
                dir.path(),
                vec!["assets/a.svg", "assets/new.svg", executable_name()],
            );
            fs::create_dir_all(job.root.join("assets")).unwrap();
            fs::create_dir_all(job.staging.join("backup/assets")).unwrap();
            fs::write(job.root.join("assets/a.svg"), "old asset").unwrap();
            fs::write(job.root.join(executable_name()), "old binary").unwrap();
            let mut journal = Journal::default();
            for relative in job.files.iter().take(interrupted_after) {
                let destination = target(&job, relative);
                let original = destination.exists();
                if original {
                    fs::copy(&destination, job.staging.join("backup").join(relative)).unwrap();
                }
                journal.changes.push(Change {
                    relative: relative.clone(),
                    original,
                });
                durable_json(&job.staging.join("transaction.json"), &journal).unwrap();
                fs::write(destination, "new file").unwrap();
            }
            rollback(&job).unwrap();
            rollback(&job).unwrap();
            assert_eq!(
                fs::read_to_string(job.root.join("assets/a.svg")).unwrap(),
                "old asset"
            );
            assert_eq!(
                fs::read_to_string(job.root.join(executable_name())).unwrap(),
                "old binary"
            );
            assert!(!job.root.join("assets/new.svg").exists());
        }
    }
    #[test]
    fn checks_hash_format() {
        assert!(valid_hash(&"a".repeat(64)).is_ok());
        assert!(valid_hash("sha256:bad").is_err());
    }
    #[test]
    fn only_one_helper_can_replace_program_files() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join(".updates")).unwrap();
        let first = coordination_lock(directory.path()).unwrap();
        assert!(coordination_lock(directory.path()).is_err());
        drop(first);
        assert!(coordination_lock(directory.path()).is_ok());
    }
    #[test]
    fn rejects_traversal_and_protected_files_inside_archives() {
        for name in [
            "bundle/../data/instances.json",
            "bundle/libretranslate-desktop.json",
            "bundle/assets/../../secret",
            "bundle/assets/C:stream",
        ] {
            let directory = tempfile::tempdir().unwrap();
            let archive = directory.path().join("release.zip");
            let mut writer = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"invalid").unwrap();
            writer.finish().unwrap();
            let out = directory.path().join("out");
            fs::create_dir(&out).unwrap();
            assert!(extract(&archive, &out, "bundle", true).is_err(), "{name}");
            assert!(!directory.path().join("data/instances.json").exists());
        }
    }

    fn zip_bundle(method: zip::CompressionMethod) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, content) in [
            (
                format!("bundle/{}", executable_name()),
                b"new executable".as_slice(),
            ),
            (
                "bundle/assets/日本語.txt".into(),
                b"translated asset".as_slice(),
            ),
        ] {
            writer
                .start_file(
                    name,
                    zip::write::SimpleFileOptions::default().compression_method(method),
                )
                .unwrap();
            writer.write_all(content).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn extracts_stored_and_deflated_zip_bundles_with_unicode_filenames() {
        for method in [
            zip::CompressionMethod::Stored,
            zip::CompressionMethod::Deflated,
        ] {
            let directory = tempfile::tempdir().unwrap();
            let archive = directory.path().join("release.zip");
            fs::write(&archive, zip_bundle(method)).unwrap();
            let out = directory.path().join("out");
            let files = extract(&archive, &out, "bundle", true).unwrap();
            assert_eq!(files, [executable_name(), "assets/日本語.txt"]);
            assert_eq!(
                fs::read(out.join(executable_name())).unwrap(),
                b"new executable"
            );
            assert_eq!(
                fs::read(out.join("assets/日本語.txt")).unwrap(),
                b"translated asset"
            );
        }
    }

    #[test]
    fn extracts_legacy_cp437_zip_filenames() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("release.zip");
        let mut bytes = zip_bundle(zip::CompressionMethod::Stored);
        let name = "bundle/assets/日本語.txt".as_bytes();
        let offsets = bytes
            .windows(name.len())
            .enumerate()
            .filter_map(|(index, part)| (part == name).then_some(index))
            .collect::<Vec<_>>();
        assert_eq!(
            offsets.len(),
            2,
            "both local and central filenames must be changed"
        );
        let legacy_name = b"bundle/assets/\x82-legacy!.txt";
        assert_eq!(name.len(), legacy_name.len());
        for offset in offsets {
            bytes[offset..offset + name.len()].copy_from_slice(legacy_name);
        }
        fs::write(&archive, bytes).unwrap();
        let out = directory.path().join("out");
        let files = extract(&archive, &out, "bundle", true).unwrap();
        assert_eq!(files, [executable_name(), "assets/é-legacy!.txt"]);
        assert_eq!(
            fs::read(out.join("assets/é-legacy!.txt")).unwrap(),
            b"translated asset"
        );
    }

    #[test]
    fn rejects_zip_bundles_with_corrupted_file_content() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("release.zip");
        let mut bytes = zip_bundle(zip::CompressionMethod::Stored);
        let payload = b"new executable";
        let offset = bytes
            .windows(payload.len())
            .position(|part| part == payload)
            .unwrap();
        bytes[offset] ^= 1;
        fs::write(&archive, bytes).unwrap();
        assert!(extract(&archive, &directory.path().join("out"), "bundle", true).is_err());
    }

    #[test]
    fn rejects_zip_symlinks_before_creating_the_link() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("release.zip");
        let mut writer = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
        writer
            .add_symlink(
                "bundle/assets/link",
                "../../private",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer.finish().unwrap();
        let out = directory.path().join("out");
        let error = extract(&archive, &out, "bundle", true).unwrap_err();
        assert!(error.to_string().contains("Symlink in update"), "{error:#}");
        assert!(fs::symlink_metadata(out.join("assets/link")).is_err());
    }
}
