use crate::downloads::{DownloadPhase, ModelDownload, RuntimeTransfers};
use crate::{
    model::{Device, Instance},
    settings::StoragePaths,
};
use anyhow::{Context, Result, bail, ensure};
use reqwest::blocking::Client;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

pub const LIBRETRANSLATE_VERSION: &str = crate::releases::DEFAULT_VERSION;
const UV_VERSION: &str = "0.13.8";
pub type Logger = Arc<dyn Fn(String) + Send + Sync>;

pub fn instance_dir(root: &Path, id: uuid::Uuid) -> PathBuf {
    root.join("instances").join(id.to_string())
}

pub fn python(dir: &Path) -> PathBuf {
    let dir =
        environment_directory(&active_runtime(dir).unwrap_or_else(|_| dir.join("invalid-runtime")));
    if cfg!(windows) {
        dir.join("Scripts/python.exe")
    } else {
        dir.join("bin/python")
    }
}
fn environment_directory(runtime: &Path) -> PathBuf {
    if runtime.join(".gui-runtime").is_file() {
        runtime.to_path_buf()
    } else {
        runtime.join("venv")
    }
}

pub fn installed(dir: &Path) -> bool {
    installed_version(dir).is_some()
}
pub fn installed_version(dir: &Path) -> Option<String> {
    let version = fs::read_to_string(active_runtime(dir).ok()?.join("installed-version"))
        .ok()?
        .trim()
        .to_string();
    (python(dir).is_file() && crate::releases::supported(&version)).then_some(version)
}
pub fn selected_ready(dir: &Path, instance: &Instance) -> bool {
    ready(dir, instance.device)
        && installed_version(dir).as_deref()
            == Some(instance.server.libretranslate_version.as_str())
}

pub fn ready(dir: &Path, device: Device) -> bool {
    installed(dir)
        && (device == Device::Cpu
            || fs::read_to_string(
                active_runtime(dir)
                    .unwrap_or_default()
                    .join("cuda-installed"),
            )
            .is_ok_and(|version| version == "12.8"))
}

fn runtime_name(name: &str) -> bool {
    (name.len() == 4 && name.bytes().all(|byte| byte.is_ascii_hexdigit()))
        || name
            .strip_prefix("rt-")
            .is_some_and(|id| id.len() == 16 && id.bytes().all(|byte| byte.is_ascii_hexdigit()))
        || name
            .strip_prefix("runtime-")
            .and_then(|id| uuid::Uuid::parse_str(id).ok())
            .is_some()
}

fn active_runtime(dir: &Path) -> Result<PathBuf> {
    let pointer = dir.join("runtime-active.json");
    if !pointer.exists() {
        return Ok(dir.to_path_buf());
    }
    let name: String = serde_json::from_slice(&fs::read(pointer)?)?;
    ensure!(runtime_name(&name), "Invalid runtime selection");
    let path = dir.join(name);
    ensure!(
        path.file_name().is_some_and(|name| name.len() != 4) || path.join(".gui-runtime").is_file(),
        "Unowned runtime folder"
    );
    let metadata = fs::symlink_metadata(&path)?;
    ensure!(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && crate::paths::same_path(
                dunce::canonicalize(&path)?
                    .parent()
                    .context("Missing runtime parent")?,
                &dunce::canonicalize(dir)?
            ),
        "Invalid runtime folder"
    );
    Ok(path)
}

fn commit_runtime(dir: &Path, stage: &Path) -> Result<()> {
    let name = stage
        .file_name()
        .context("Missing runtime name")?
        .to_string_lossy();
    let mut pointer = tempfile::NamedTempFile::new_in(dir)?;
    use std::io::Write;
    pointer.write_all(&serde_json::to_vec(name.as_ref())?)?;
    pointer.as_file().sync_all()?;
    pointer.persist(dir.join("runtime-active.json"))?;
    Ok(())
}

fn clean_runtimes(dir: &Path, previous: &Path, active: &Path) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if runtime_name(&name)
            && (name.len() != 4 || entry.path().join(".gui-runtime").is_file())
            && entry.path() != previous
            && entry.path() != active
            && entry.file_type()?.is_dir()
            && !entry.file_type()?.is_symlink()
            && crate::paths::same_path(
                dunce::canonicalize(entry.path())?
                    .parent()
                    .context("Missing runtime parent")?,
                &dunce::canonicalize(dir)?,
            )
        {
            fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}

pub fn wait_for_port_close(port: u16, timeout: Duration) -> bool {
    let address = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port));
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if std::net::TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_err() {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(75));
    }
}

pub fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Assign the process to its job before any Python launcher can create children.
        cmd.creation_flags(0x08000000 | 0x00000004); // CREATE_NO_WINDOW | CREATE_SUSPENDED
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.stdin(Stdio::null());
    cmd
}

pub fn configure(cmd: &mut Command, paths: &StoragePaths, dir: &Path) -> Result<()> {
    let dir = dunce::simplified(dir);
    for (key, _) in std::env::vars_os().filter(|(key, _)| {
        let key = key.to_string_lossy().to_ascii_uppercase();
        key.starts_with("LT_") || key.starts_with("PYTHON")
    }) {
        cmd.env_remove(key);
    }
    for child in [
        "data",
        "cache",
        "config",
        "packages",
        "db",
        "data/prometheus",
    ] {
        fs::create_dir_all(dir.join(child))?;
    }
    cmd.current_dir(dir)
        .env_remove("PYTHONHOME")
        .env_remove("PYTHONPATH")
        .env_remove("PYTHONOPTIMIZE")
        .env_remove("PYTHONINSPECT")
        .env_remove("PYTHONSTARTUP")
        .env("UV_PYTHON_INSTALL_DIR", dunce::simplified(&paths.python))
        .env("UV_CACHE_DIR", dunce::simplified(&paths.cache))
        .env("UV_PYTHON_PREFERENCE", "only-managed")
        .env("UV_NO_PROGRESS", "1")
        .env("NO_COLOR", "1")
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONUTF8", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONNOUSERSITE", "1")
        .env("XDG_DATA_HOME", dir.join("data"))
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("ARGOS_PACKAGES_DIR", dir.join("packages"))
        .env("PROMETHEUS_MULTIPROC_DIR", dir.join("data/prometheus"));
    let runtime_dir = environment_directory(&active_runtime(dir)?);
    let site = if cfg!(windows) {
        runtime_dir.join("Lib/site-packages")
    } else {
        runtime_dir.join("lib/python3.11/site-packages")
    };
    let library_dirs = ["cublas", "cuda_runtime"].map(|name| {
        site.join("nvidia")
            .join(name)
            .join(if cfg!(windows) { "bin" } else { "lib" })
    });
    let mut search: Vec<PathBuf> = library_dirs
        .into_iter()
        .filter(|path| path.is_dir())
        .collect();
    let variable = if cfg!(windows) {
        "PATH"
    } else {
        "LD_LIBRARY_PATH"
    };
    if let Some(existing) = std::env::var_os(variable) {
        search.extend(std::env::split_paths(&existing));
    }
    if !search.is_empty() {
        cmd.env(variable, std::env::join_paths(search)?);
    }
    Ok(())
}

pub struct OwnedChild(pub Child, #[cfg(windows)] std::os::windows::io::OwnedHandle);
impl OwnedChild {
    fn spawn(cmd: &mut Command) -> Result<Self> {
        let child = cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
        #[cfg(windows)]
        {
            use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
            use windows_sys::Win32::System::JobObjects::*;
            let result = (|| -> Result<OwnedHandle> {
                // Closing the job terminates runtime/build subprocesses too.
                let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
                ensure!(
                    !handle.is_null(),
                    "Could not create a runtime process job: {}",
                    std::io::Error::last_os_error()
                );
                let job = unsafe { OwnedHandle::from_raw_handle(handle) };
                let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION =
                    unsafe { std::mem::zeroed() };
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                ensure!(
                    unsafe {
                        SetInformationJobObject(
                            handle,
                            JobObjectExtendedLimitInformation,
                            &limits as *const _ as *const _,
                            std::mem::size_of_val(&limits) as u32,
                        )
                    } != 0,
                    "Could not configure runtime process cleanup."
                );
                ensure!(
                    unsafe { AssignProcessToJobObject(handle, child.as_raw_handle()) } != 0,
                    "Could not supervise the runtime process: {}",
                    std::io::Error::last_os_error()
                );
                resume_process(child.id())?;
                Ok(job)
            })();
            match result {
                Ok(job) => Ok(Self(child, job)),
                Err(error) => {
                    let mut child = child;
                    let _ = child.kill();
                    let _ = child.wait();
                    Err(error)
                }
            }
        }
        #[cfg(not(windows))]
        {
            Ok(Self(child))
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            unsafe {
                windows_sys::Win32::System::JobObjects::TerminateJobObject(
                    self.1.as_raw_handle(),
                    1,
                );
            }
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::System::JobObjects::*;
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION =
                    unsafe { std::mem::zeroed() };
                let ok = unsafe {
                    QueryInformationJobObject(
                        self.1.as_raw_handle(),
                        JobObjectBasicAccountingInformation,
                        &mut info as *mut _ as *mut _,
                        std::mem::size_of_val(&info) as u32,
                        std::ptr::null_mut(),
                    )
                };
                if ok == 0 || info.ActiveProcesses == 0 || std::time::Instant::now() >= deadline {
                    break;
                }
                thread::sleep(Duration::from_millis(30));
            }
        }
    }
}

#[cfg(windows)]
fn resume_process(pid: u32) -> Result<()> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        System::{Diagnostics::ToolHelp::*, Threading::*},
    };
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    ensure!(
        snapshot != INVALID_HANDLE_VALUE,
        "Could not inspect the suspended runtime process."
    );
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot) };
    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of_val(&entry) as u32;
    let mut found = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) };
    while found != 0 {
        if entry.th32OwnerProcessID == pid {
            let handle = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
            ensure!(!handle.is_null(), "Could not open the runtime thread.");
            let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
            ensure!(
                unsafe { ResumeThread(handle.as_raw_handle()) } != u32::MAX,
                "Could not resume the supervised runtime."
            );
            return Ok(());
        }
        found = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) };
    }
    bail!("The suspended runtime thread could not be found.")
}

pub fn pipe_logs(child: &mut Child, log: &Logger) {
    let _ = log_readers(child, log);
}
fn log_readers(child: &mut Child, log: &Logger) -> Vec<thread::JoinHandle<()>> {
    fn drain(stream: impl Read + Send + 'static, log: Logger) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            let mut reader = BufReader::new(stream);
            loop {
                // Bound even a single unterminated line from a dependency.
                let mut line = Vec::new();
                match reader.by_ref().take(8192).read_until(b'\n', &mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => log(String::from_utf8_lossy(&line).trim_end().to_string()),
                }
            }
        })
    }
    let mut readers = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        readers.push(drain(stdout, log.clone()));
    }
    if let Some(stderr) = child.stderr.take() {
        readers.push(drain(stderr, log.clone()));
    }
    readers
}

fn run(mut cmd: Command, cancel: &AtomicBool, log: &Logger) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "Installation cancelled.");
    let mut child = OwnedChild::spawn(&mut cmd).context("Could not launch the managed runtime")?;
    let transfers = Arc::new(std::sync::Mutex::new(RuntimeTransfers::default()));
    let tracked = transfers.clone();
    let downstream = log.clone();
    let logger: Logger = Arc::new(move |line| {
        if let Some(progress) = tracked.lock().unwrap().observe_uv(&line) {
            downstream(progress.message());
        }
        downstream(line);
    });
    let readers = log_readers(&mut child.0, &logger);
    let result = (|| {
        loop {
            ensure!(!cancel.load(Ordering::Relaxed), "Installation cancelled.");
            if let Some(status) = child.0.try_wait()? {
                ensure!(
                    status.success(),
                    "Installation command exited with {status}. See the log for details."
                );
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
    })();
    // Kill descendants before joining readers, including on cancellation. Drain all
    // progress before marking tasks finished so late output cannot reopen a task.
    drop(child);
    for reader in readers {
        let _ = reader.join();
    }
    let phase = if cancel.load(Ordering::Relaxed) {
        DownloadPhase::Interrupted
    } else if result.is_ok() {
        DownloadPhase::Complete
    } else {
        DownloadPhase::Failed
    };
    for progress in transfers.lock().unwrap().finish(phase) {
        log(progress.message());
    }
    result
}

fn uv_target() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => Ok("aarch64-pc-windows-msvc"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        _ => bail!("This operating system or CPU is not supported by the runtime installer."),
    }
}

fn download(
    client: &Client,
    url: &str,
    max: u64,
    cancel: &AtomicBool,
    progress: impl Fn(u64, Option<u64>),
) -> Result<Vec<u8>> {
    ensure!(!cancel.load(Ordering::Relaxed), "Installation cancelled.");
    let mut response = client.get(url).send()?.error_for_status()?;
    let total = response.content_length().filter(|size| *size > 0);
    ensure!(
        total.is_none_or(|size| size <= max),
        "Download exceeds the expected size limit."
    );
    progress(0, total);
    let mut bytes = Vec::new();
    let mut chunk = [0; 65536];
    loop {
        ensure!(!cancel.load(Ordering::Relaxed), "Installation cancelled.");
        let read = response.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..read]);
        ensure!(
            bytes.len() as u64 <= max,
            "Download exceeds the expected size limit."
        );
        progress(bytes.len() as u64, total);
    }
    ensure!(
        bytes.len() as u64 <= max,
        "Download exceeds the expected size limit."
    );
    Ok(bytes)
}

fn ensure_uv(root: &Path, cancel: &AtomicBool, log: &Logger) -> Result<PathBuf> {
    let bin = root
        .join("tools")
        .join(format!("uv-{UV_VERSION}{}", std::env::consts::EXE_SUFFIX));
    if bin.is_file() {
        return Ok(bin);
    }
    log("Downloading the private runtime installer…".into());
    let name = format!("uv-{UV_VERSION}");
    let downloaded = std::cell::Cell::new(0);
    let total = std::cell::Cell::new(None);
    log(ModelDownload::runtime(&name, 0, None, DownloadPhase::Queued).message());
    let result = (|| {
        let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
        let asset = format!("uv-{}.{}", uv_target()?, ext);
        let base =
            format!("https://github.com/astral-sh/uv/releases/download/{UV_VERSION}/{asset}");
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(60))
            .user_agent("LibreTranslate-Desktop/0.1")
            .build()?;
        let archive = download(&client, &base, 100 * 1024 * 1024, cancel, |bytes, size| {
            downloaded.set(bytes);
            total.set(size);
            log(ModelDownload::runtime(&name, bytes, size, DownloadPhase::Downloading).message());
        })?;
        ensure!(!cancel.load(Ordering::Relaxed), "Installation cancelled.");
        log(ModelDownload::runtime(
            &name,
            downloaded.get(),
            total.get(),
            DownloadPhase::Validating,
        )
        .message());
        let checksum = download(&client, &format!("{base}.sha256"), 4096, cancel, |_, _| {})?;
        verify_checksum(&archive, &String::from_utf8(checksum)?)?;
        let name = if cfg!(windows) { "uv.exe" } else { "uv" };
        let mut executable = Vec::new();
        if cfg!(windows) {
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive))?;
            for index in 0..zip.len() {
                let entry = zip.by_index(index)?;
                if Path::new(entry.name())
                    .file_name()
                    .is_some_and(|n| n == name)
                {
                    entry.take(100 * 1024 * 1024).read_to_end(&mut executable)?;
                    break;
                }
            }
        } else {
            let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(&archive[..]));
            for entry in tar.entries()? {
                let entry = entry?;
                if entry.path()?.file_name().is_some_and(|n| n == name) {
                    entry.take(100 * 1024 * 1024).read_to_end(&mut executable)?;
                    break;
                }
            }
        }
        ensure!(
            !executable.is_empty(),
            "The runtime archive did not contain {name}."
        );
        ensure!(!cancel.load(Ordering::Relaxed), "Installation cancelled.");
        fs::create_dir_all(bin.parent().unwrap())?;
        let temp = bin.with_extension("download");
        fs::write(&temp, executable)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temp, fs::Permissions::from_mode(0o755))?;
        }
        fs::rename(temp, &bin)?;
        Ok(bin)
    })();
    let phase = if cancel.load(Ordering::Relaxed) {
        DownloadPhase::Interrupted
    } else if result.is_ok() {
        DownloadPhase::Complete
    } else {
        DownloadPhase::Failed
    };
    log(ModelDownload::runtime(&name, downloaded.get(), total.get(), phase).message());
    result
}

fn verify_checksum(bytes: &[u8], checksum: &str) -> Result<()> {
    let expected = checksum
        .split_whitespace()
        .next()
        .context("Missing archive checksum")?;
    ensure!(
        expected.len() == 64 && expected.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid archive checksum."
    );
    ensure!(
        format!("{:x}", Sha256::digest(bytes)).eq_ignore_ascii_case(expected),
        "Runtime download checksum mismatch. Retry the installation."
    );
    Ok(())
}

fn create_runtime_stage(dir: &Path) -> Result<PathBuf> {
    // Keep the full Python/DLL path no longer than the legacy `venv` layout.
    // Names are not secrets; exclusive creation handles collisions safely.
    for _ in 0..32 {
        let stage = dir.join(format!("{:04x}", uuid::Uuid::new_v4().as_u128() as u16));
        match fs::create_dir(&stage) {
            Ok(()) => {
                fs::write(stage.join(".gui-runtime"), "1")?;
                return Ok(stage);
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    bail!("Could not reserve a private runtime folder")
}

pub fn install(
    paths: &StoragePaths,
    instance: &Instance,
    cancel: &AtomicBool,
    log: &Logger,
) -> Result<()> {
    let root = &paths.root;
    let dir = instance_dir(root, instance.id);
    fs::create_dir_all(&dir)?;
    let previous = active_runtime(&dir)?;
    let stage = create_runtime_stage(&dir)?;
    let result = install_staged(paths, instance, &dir, &stage, cancel, log);
    if result.is_err() {
        // Deleting a Python environment can take minutes on Windows. It must not
        // delay reporting failure or prevent shutdown; an interrupted cleanup is
        // retried by retention after a later successful installation.
        let failed = stage.clone();
        let owner = dir.clone();
        let _ = thread::Builder::new()
            .name("runtime-staging-cleanup".into())
            .spawn(move || {
                if active_runtime(&owner).is_ok_and(|active| active != failed)
                    && dunce::canonicalize(&failed).is_ok_and(|path| {
                        path.parent().is_some_and(|parent| {
                            dunce::canonicalize(&owner)
                                .is_ok_and(|root| crate::paths::same_path(parent, &root))
                        })
                    })
                {
                    let _ = fs::remove_dir_all(failed);
                }
            });
        return result;
    }
    commit_runtime(&dir, &stage)?;
    if let Err(error) = clean_runtimes(&dir, &previous, &stage) {
        log(format!("Runtime cleanup deferred: {error:#}"));
    }
    log("Runtime and selected language models installed.".into());
    Ok(())
}

fn install_staged(
    paths: &StoragePaths,
    instance: &Instance,
    dir: &Path,
    stage: &Path,
    cancel: &AtomicBool,
    log: &Logger,
) -> Result<()> {
    let root = &paths.root;
    let uv = ensure_uv(root, cancel, log)?;
    log("Preparing a private Python 3.11 environment…".into());
    if !python(stage).exists() {
        let mut cmd = command(&uv);
        configure(&mut cmd, paths, dir)?;
        cmd.args(["venv", "--allow-existing", "--python", "3.11"])
            .arg(stage);
        run(cmd, cancel, log)?;
    }
    log(format!(
        "Installing LibreTranslate {}. The first download can take several minutes…",
        instance.server.libretranslate_version
    ));
    let mut cmd = command(&uv);
    configure(&mut cmd, paths, dir)?;
    cmd.args(["pip", "install", "--python"])
        .arg(python(stage))
        .arg(format!(
            "libretranslate=={}",
            instance.server.libretranslate_version
        ))
        .arg("minisbd==0.9.5")
        .arg("ctranslate2==4.9.0")
        .arg("chardet==5.2.0");
    run(cmd, cancel, log)?;
    if instance.device == Device::Cuda {
        ensure!(
            crate::gpu::detect().available(),
            "No supported NVIDIA GPU was detected. Select CPU or install an NVIDIA driver."
        );
        log("Installing private CUDA 12 libraries. The NVIDIA display driver must already be installed.".into());
        let mut cmd = command(&uv);
        configure(&mut cmd, paths, dir)?;
        cmd.args(["pip", "install", "--python"])
            .arg(python(stage))
            .args([
                "nvidia-cublas-cu12==12.8.4.1",
                "nvidia-cuda-runtime-cu12==12.8.90",
            ]);
        run(cmd, cancel, log)?;
        fs::write(stage.join("cuda-installed"), "12.8")?;
    }
    let mut cmd = command(python(stage));
    configure(&mut cmd, paths, dir)?;
    cmd.args(["-c","import sys; from importlib.metadata import version; from libretranslate.main import get_parser; get_parser().parse_args([]); assert version('libretranslate') == sys.argv[1], 'Installed LibreTranslate version does not match the selection'"])
        .arg(&instance.server.libretranslate_version);
    run(cmd, cancel, log)?;
    log("Installing selected language models…".into());
    recover_model_transactions(dir)?;
    let mut cmd = command(python(stage));
    configure(&mut cmd, paths, dir)?;
    cmd.arg("-c")
        .arg(format!(
            "{CUDA_BOOTSTRAP}\n{API_KEY_BOOTSTRAP}\n{MODEL_MANAGEMENT}\nsync_models(sys.argv[1].split(','), prune=True)"
        ))
        .arg(&instance.languages);
    run(cmd, cancel, log)?;
    fs::write(
        stage.join("installed-version"),
        &instance.server.libretranslate_version,
    )?;
    Ok(())
}

pub fn start(paths: &StoragePaths, instance: &Instance, log: &Logger) -> Result<OwnedChild> {
    let root = &paths.root;
    let dir = instance_dir(root, instance.id);
    recover_model_transactions(&dir)?;
    ensure!(
        selected_ready(&dir, instance),
        "Install the selected LibreTranslate version before starting this instance."
    );
    // A bind probe wrongly rejects recently stopped servers in TIME_WAIT on Windows.
    // Check for a live listener; LibreTranslate will report any remaining bind failure.
    let address = std::net::SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, instance.port));
    ensure!(
        std::net::TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_err(),
        "Port {} is in use. Stop the other server or choose another port.",
        instance.port
    );
    let mut cmd = command(python(&dir));
    configure(&mut cmd, paths, &dir)?;
    cmd.env(
        "ARGOS_DEVICE_TYPE",
        if instance.device == Device::Cuda {
            "cuda"
        } else {
            "cpu"
        },
    );
    if instance.device == Device::Cuda {
        ensure!(
            crate::gpu::detect().available(),
            "No supported NVIDIA GPU was detected. Switch this instance to CPU."
        );
        let mut probe = command(python(&dir));
        configure(&mut probe, paths, &dir)?;
        probe.args(["-c", &format!("{CUDA_BOOTSTRAP}\nimport ctranslate2\nassert ctranslate2.get_cuda_device_count() > 0, 'No CUDA device is available'\nassert ctranslate2.get_supported_compute_types('cuda'), 'CUDA runtime is not ready'")]);
        let output = capture(&mut probe, Duration::from_secs(10))?;
        ensure!(
            output.0,
            "CUDA is not ready. Repair the runtime, update the NVIDIA driver, or choose CPU. {}",
            output.1
        );
    }
    cmd.args([
        "-c",
        &server_script(),
        "--host",
        if instance.server.lan_access {
            "0.0.0.0"
        } else {
            "127.0.0.1"
        },
        "--port",
    ])
    .arg(instance.port.to_string())
    .arg("--load-only")
    .arg(&instance.languages)
    .arg("--threads")
    .arg(
        instance
            .threads
            .min(crate::model::max_threads())
            .to_string(),
    );
    if instance.server.metrics {
        clean_metrics(&dir)?;
        let metrics = dunce::simplified(&dir)
            .join("data/prometheus")
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&metrics)?;
        cmd.env("PROMETHEUS_MULTIPROC_DIR", metrics);
    }
    cmd.args(instance.server.args());
    cmd.env("LT_METRICS_AUTH_TOKEN", &instance.server.metrics_token);
    let key_database = dunce::simplified(&dir).join("data/api_keys.db");
    cmd.arg("--api-keys-db-path").arg(&key_database);
    if instance.server.api_keys
        || instance.server.require_api_key
        || instance.server.request_limit == Some(0)
    {
        cmd.env("LIBRETRANSLATE_GUI_API_KEY", &instance.server.api_key)
            .env("LIBRETRANSLATE_GUI_API_DB", &key_database)
            .env(
                "LIBRETRANSLATE_GUI_KEY_LIMIT",
                instance
                    .server
                    .request_limit
                    .map(i64::from)
                    .unwrap_or(-1)
                    .to_string(),
            );
    }
    let mut child = OwnedChild::spawn(&mut cmd)?;
    pipe_logs(&mut child.0, log);
    log(
        "Starting server. Synchronizing selected language models; downloads can take several minutes."
            .into(),
    );
    Ok(child)
}

const API_KEY_BOOTSTRAP: &str = include_str!("../assets/runtime-bootstrap.py");
const MODEL_MANAGEMENT: &str = include_str!("../assets/update-models.py");
fn clean_metrics(dir: &Path) -> Result<()> {
    let root = dir.join("data/prometheus");
    if !root.is_dir() {
        return Ok(());
    }
    let canonical = dunce::canonicalize(&root)?;
    ensure!(
        crate::paths::same_path(
            &canonical,
            &dunce::canonicalize(dir)?.join("data/prometheus")
        ),
        "Invalid metrics folder"
    );
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if entry
            .file_name()
            .to_str()
            .and_then(|name| uuid::Uuid::parse_str(name).ok())
            .is_some()
            && entry.file_type()?.is_dir()
            && !entry.file_type()?.is_symlink()
        {
            ensure!(
                crate::paths::same_path(
                    dunce::canonicalize(entry.path())?
                        .parent()
                        .context("Missing metrics parent")?,
                    &canonical
                ),
                "Invalid metrics run folder"
            );
            fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}

pub fn clean_storage(
    paths: &StoragePaths,
    instance: &Instance,
    cancel: &AtomicBool,
    log: &Logger,
) -> Result<()> {
    let dir = instance_dir(&paths.root, instance.id);
    recover_model_transactions(&dir)?;
    clean_metrics(&dir)?;
    let mut cmd = command(python(&dir));
    configure(&mut cmd, paths, &dir)?;
    cmd.arg("-c").arg(format!("{CUDA_BOOTSTRAP}\n{API_KEY_BOOTSTRAP}\n{MODEL_MANAGEMENT}\n_root = Path(os.environ['ARGOS_PACKAGES_DIR']).resolve()\n_transactions = _root.parent / 'model-transactions'\nif _transactions.is_dir(): _retention(_root, _transactions)\nfor _archive in Path(settings.downloads_dir).glob('*.argosmodel'):\n if _archive.is_file() and not _archive.is_symlink(): _archive.unlink()\nprint('Storage cleanup complete. The latest model backup was kept.', flush=True)"));
    run(cmd, cancel, log)
}
fn server_script() -> String {
    format!(
        "{CUDA_BOOTSTRAP}\n{API_KEY_BOOTSTRAP}\n{MODEL_MANAGEMENT}\n_gui_selected = sys.argv[sys.argv.index('--load-only') + 1].split(',')\n_gui_update = '--update-models' in sys.argv\nif _gui_update: sys.argv.remove('--update-models')\nsync_models(_gui_selected, update=_gui_update, prune=True)\nfrom libretranslate.main import main\nmain()"
    )
}
pub fn recover_model_transactions(dir: &Path) -> Result<()> {
    let transactions = dir.join("model-transactions");
    let Ok(entries) = fs::read_dir(&transactions) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let manifest = entry.path().join("destination.json");
        if !manifest.is_file() {
            continue;
        }
        let Ok(name) = serde_json::from_slice::<String>(&fs::read(manifest)?) else {
            continue;
        };
        let mut components = Path::new(&name).components();
        ensure!(
            matches!(components.next(), Some(std::path::Component::Normal(_)))
                && components.next().is_none(),
            "Invalid model recovery path."
        );
        let target = dir.join("packages").join(name);
        let old = entry.path().join("old");
        if !target.exists() && old.is_dir() {
            fs::rename(old, target).context("Could not restore an interrupted model update")?;
        }
    }
    Ok(())
}

pub fn update_models(
    paths: &StoragePaths,
    instance: &Instance,
    cancel: &AtomicBool,
    log: &Logger,
) -> Result<()> {
    let dir = instance_dir(&paths.root, instance.id);
    ensure!(
        installed(&dir),
        "Install the runtime before updating models."
    );
    let mut cmd = command(python(&dir));
    configure(&mut cmd, paths, &dir)?;
    recover_model_transactions(&dir)?;
    cmd.arg("-c")
        .arg(format!(
            "{CUDA_BOOTSTRAP}\n{API_KEY_BOOTSTRAP}\n{MODEL_MANAGEMENT}\nsync_models(sys.argv[1].split(','), update=True)"
        ))
        .arg(&instance.languages);
    log("Checking for model updates…".into());
    run(cmd, cancel, log)?;
    Ok(())
}

// Python 3.8+ does not use PATH for all extension DLL dependencies on Windows.
const CUDA_BOOTSTRAP: &str = "import os, sys, glob\n_cuda_dll_handles = [os.add_dll_directory(p) for p in glob.glob(os.path.join(sys.prefix, 'Lib', 'site-packages', 'nvidia', '*', 'bin'))] if sys.platform == 'win32' else []";

pub fn capture(cmd: &mut Command, timeout: Duration) -> Result<(bool, String)> {
    let mut child = OwnedChild::spawn(cmd)?;
    let stdout = child.0.stdout.take();
    let stderr = child.0.stderr.take();
    let (send, receive) = std::sync::mpsc::channel();
    for stream in [
        stdout.map(|s| Box::new(s) as Box<dyn Read + Send>),
        stderr.map(|s| Box::new(s) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    {
        let send = send.clone();
        thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stream.take(32768).read_to_end(&mut bytes);
            let _ = send.send(bytes);
        });
    }
    drop(send);
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.0.try_wait()? {
            break status;
        }
        ensure!(
            std::time::Instant::now() < deadline,
            "Hardware check timed out."
        );
        thread::sleep(Duration::from_millis(40));
    };
    let mut output = String::new();
    for _ in 0..2 {
        if let Ok(bytes) = receive.recv_timeout(Duration::from_secs(1)) {
            output.push_str(&String::from_utf8_lossy(&bytes));
        }
    }
    Ok((status.success(), output))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_archive_progress_reports_measured_bytes_with_and_without_content_length() {
        for known_size in [true, false] {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let server = thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = [0; 4096];
                assert!(socket.read(&mut request).unwrap() > 0);
                use std::io::Write;
                let length = if known_size {
                    "Content-Length: 8\r\n"
                } else {
                    ""
                };
                write!(
                    socket,
                    "HTTP/1.1 200 OK\r\n{length}Connection: close\r\n\r\n12345678"
                )
                .unwrap();
            });
            let progress = std::cell::RefCell::new(Vec::new());
            let client = Client::builder()
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap();
            let bytes = download(
                &client,
                &format!("http://{address}/uv.zip"),
                16,
                &AtomicBool::new(false),
                |bytes, total| progress.borrow_mut().push((bytes, total)),
            )
            .unwrap();
            server.join().unwrap();
            assert_eq!(bytes, b"12345678");
            let progress = progress.into_inner();
            assert_eq!(progress.first(), Some(&(0, known_size.then_some(8))));
            assert_eq!(progress.last(), Some(&(8, known_size.then_some(8))));
        }
    }
    #[test]
    fn subprocess_download_tasks_finish_after_the_final_output_is_drained() {
        for success in [true, false] {
            #[cfg(windows)]
            let mut cmd = command("cmd");
            #[cfg(windows)]
            cmd.args([
                "/D",
                "/C",
                if success {
                    "echo Downloading nvidia-cublas-cu12 & exit /B 0"
                } else {
                    "echo Downloading nvidia-cublas-cu12 & exit /B 1"
                },
            ]);
            #[cfg(unix)]
            let mut cmd = command("sh");
            #[cfg(unix)]
            cmd.args([
                "-c",
                if success {
                    "printf 'Downloading nvidia-cublas-cu12\\n'; exit 0"
                } else {
                    "printf 'Downloading nvidia-cublas-cu12\\n'; exit 1"
                },
            ]);
            let messages = Arc::new(std::sync::Mutex::new(Vec::new()));
            let saved = messages.clone();
            let log: Logger = Arc::new(move |line| saved.lock().unwrap().push(line));
            assert_eq!(run(cmd, &AtomicBool::new(false), &log).is_ok(), success);
            let progress = messages
                .lock()
                .unwrap()
                .iter()
                .filter_map(|line| ModelDownload::parse(line))
                .collect::<Vec<_>>();
            assert_eq!(progress.len(), 2);
            assert_eq!(progress[0].phase, DownloadPhase::Downloading);
            assert_eq!(progress[1].source, "nvidia-cublas-cu12");
            assert_eq!(
                progress[1].phase,
                if success {
                    DownloadPhase::Complete
                } else {
                    DownloadPhase::Failed
                }
            );
        }
    }
    #[test]
    fn staged_runtime_does_not_replace_working_environment_until_commit() {
        let root = tempfile::tempdir().unwrap();
        let old_python = python(root.path());
        fs::create_dir_all(old_python.parent().unwrap()).unwrap();
        fs::write(&old_python, b"old interpreter").unwrap();
        fs::write(
            root.path().join("installed-version"),
            LIBRETRANSLATE_VERSION,
        )
        .unwrap();
        let stage = create_runtime_stage(root.path()).unwrap();
        assert_eq!(python(root.path()), old_python);
        assert!(installed(root.path()));
        let new_python = python(&stage);
        assert_eq!(new_python.as_os_str().len(), old_python.as_os_str().len());
        fs::create_dir_all(new_python.parent().unwrap()).unwrap();
        fs::write(&new_python, b"new interpreter").unwrap();
        fs::write(stage.join("installed-version"), LIBRETRANSLATE_VERSION).unwrap();
        commit_runtime(root.path(), &stage).unwrap();
        assert_eq!(python(root.path()), new_python);
        assert!(installed(root.path()));
        assert_eq!(fs::read(&old_python).unwrap(), b"old interpreter");
        let failed = root
            .path()
            .join(format!("runtime-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&failed).unwrap();
        clean_runtimes(root.path(), root.path(), &stage).unwrap();
        assert!(!failed.exists());
        assert!(new_python.is_file() && old_python.is_file());
    }
    #[test]
    fn metrics_cleanup_only_removes_owned_run_folders() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("data/prometheus");
        let run = root.join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&run).unwrap();
        fs::write(run.join("counter.db"), b"old").unwrap();
        fs::write(root.join("notes.txt"), b"keep").unwrap();
        clean_metrics(dir.path()).unwrap();
        assert!(!run.exists());
        assert_eq!(fs::read(root.join("notes.txt")).unwrap(), b"keep");
    }
    #[test]
    #[ignore = "requires an explicitly prepared isolated copy of en/ro models and an existing Python environment"]
    fn isolated_real_runtime_translates_text_files_and_reports_metrics() {
        let root = PathBuf::from(
            std::env::var_os("LIBRETRANSLATE_GUI_ISOLATED_AUDIT_ROOT")
                .expect("Set the isolated audit folder"),
        );
        let root = dunce::canonicalize(root).unwrap();
        let fixture: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("fixture.json")).unwrap()).unwrap();
        assert_eq!(fixture["isolated"], true);
        let python = PathBuf::from(fixture["python"].as_str().unwrap());
        let paths = StoragePaths::new(root.clone());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let instance = Instance {
            port,
            languages: "en,ro".into(),
            threads: 2,
            server: crate::model::ServerOptions {
                url_prefix: "/audit".into(),
                api_keys: true,
                require_api_key: true,
                api_key: "audit-key".into(),
                metrics: true,
                metrics_token: "audit-metrics".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut cmd = command(python);
        configure(&mut cmd, &paths, &root).unwrap();
        cmd.env("ARGOS_DEVICE_TYPE", "cpu")
            .env("ARGOS_INTER_THREADS", "1")
            .env("ARGOS_INTRA_THREADS", "2")
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("LIBRETRANSLATE_GUI_API_KEY", "audit-key")
            .env("LIBRETRANSLATE_GUI_API_DB", root.join("data/api_keys.db"))
            .env("LIBRETRANSLATE_GUI_KEY_LIMIT", "-1")
            .env("LT_METRICS_AUTH_TOKEN", "audit-metrics");
        let metrics_directory = root
            .join("data/prometheus")
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&metrics_directory).unwrap();
        cmd.env("PROMETHEUS_MULTIPROC_DIR", metrics_directory);
        let offline = "import urllib.request\ndef _audit_offline(*args, **kwargs): raise RuntimeError('Network downloads disabled during isolated audit')\nurllib.request.urlopen = _audit_offline\n";
        cmd.arg("-c")
            .arg(format!("{offline}{}", server_script()))
            .args([
                "--host",
                "127.0.0.1",
                "--port",
                &port.to_string(),
                "--load-only",
                "en,ro",
                "--threads",
                "2",
                "--api-keys-db-path",
            ])
            .arg(root.join("data/api_keys.db"))
            .args(instance.server.args());
        let mut process = OwnedChild::spawn(&mut cmd).unwrap();
        let logs = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let captured = logs.clone();
        let logger: Logger = Arc::new(move |line| {
            println!("{line}");
            captured.lock().unwrap().push(line);
        });
        pipe_logs(&mut process.0, &logger);
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let start = std::time::Instant::now();
        while !client
            .get(format!("{}/languages", instance.url()))
            .send()
            .is_ok_and(|response| response.status().is_success())
        {
            assert!(
                process.0.try_wait().unwrap().is_none(),
                "Server stopped: {:?}",
                logs.lock().unwrap()
            );
            assert!(
                start.elapsed() < Duration::from_secs(90),
                "Server readiness timeout"
            );
            thread::sleep(Duration::from_millis(100));
        }
        let ready_ms = start.elapsed().as_millis();
        let mut samples = Vec::new();
        for (input, source, target) in [("Hello, world.", "en", "ro"), ("Salut, lume.", "ro", "en")]
        {
            let started = std::time::Instant::now();
            let mut draft = crate::translator::Draft::default();
            draft.input = input.into();
            draft.source = source.into();
            draft.target = target.into();
            let translated = crate::translator::translate(&instance, &draft).unwrap();
            assert!(!translated.text.is_empty());
            assert_ne!(translated.text, input);
            samples.push(serde_json::json!({"source":source,"target":target,"elapsed_ms":started.elapsed().as_millis(),"text":translated.text}));
        }
        let mut requests = Vec::new();
        for index in 0..6 {
            let instance = instance.clone();
            requests.push(thread::spawn(move || {
                let mut draft = crate::translator::Draft::default();
                draft.input = format!("This is test sentence number {index}.");
                draft.source = "en".into();
                draft.target = "ro".into();
                crate::translator::translate(&instance, &draft)
            }));
        }
        let mut parallel = Vec::new();
        for request in requests {
            let text = request.join().unwrap().unwrap().text;
            assert!(!text.is_empty());
            parallel.push(text);
        }
        let formats = crate::translator_files::formats(&instance).unwrap();
        assert!(formats.files_translation);
        assert!(!formats.supported_files_format.is_empty());
        let input = root.join("AUDIT-INPUT.TXT");
        let original = "Hello, world.\nI want to translate this file.\n";
        fs::write(&input, original).unwrap();
        let output = crate::translator_files::translate_file(
            &instance,
            &input,
            "en",
            "ro",
            &paths.cache.join("translations"),
        )
        .unwrap();
        let translated_path = root.join("audit-output.ro.txt");
        output.save(&translated_path).unwrap();
        let file_text = fs::read_to_string(translated_path).unwrap();
        assert!(!file_text.trim().is_empty());
        assert_ne!(file_text, original);
        assert_eq!(fs::read_to_string(input).unwrap(), original);
        let metrics = crate::statistics::fetch(&instance).unwrap();
        assert!(metrics.total.requests >= 9);
        drop(process);
        assert!(wait_for_port_close(port, Duration::from_secs(5)));
        // Compare concurrent replies to a fresh server process: an in-process
        // translation cache must not hide cross-request result corruption.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let serial_port = listener.local_addr().unwrap().port();
        drop(listener);
        cmd.args(["--port", &serial_port.to_string()]);
        let metrics_directory = root
            .join("data/prometheus")
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&metrics_directory).unwrap();
        cmd.env("PROMETHEUS_MULTIPROC_DIR", metrics_directory);
        let mut serial_instance = instance.clone();
        serial_instance.port = serial_port;
        let mut serial_process = OwnedChild::spawn(&mut cmd).unwrap();
        pipe_logs(&mut serial_process.0, &logger);
        let deadline = std::time::Instant::now() + Duration::from_secs(90);
        while !client
            .get(format!("{}/languages", serial_instance.url()))
            .send()
            .is_ok_and(|response| response.status().is_success())
        {
            assert!(serial_process.0.try_wait().unwrap().is_none());
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(Duration::from_millis(100));
        }
        for (index, expected) in parallel.iter().enumerate() {
            let mut draft = crate::translator::Draft::default();
            draft.input = format!("This is test sentence number {index}.");
            draft.source = "en".into();
            draft.target = "ro".into();
            assert_eq!(
                &crate::translator::translate(&serial_instance, &draft)
                    .unwrap()
                    .text,
                expected,
                "Concurrent response {index} differs from serial baseline"
            );
        }
        drop(serial_process);
        assert!(wait_for_port_close(serial_port, Duration::from_secs(5)));
        fs::write(root.join("result.json"),serde_json::to_vec_pretty(&serde_json::json!({"ready_ms":ready_ms,"samples":samples,"parallel_requests":6,"parallel_matches_fresh_serial":true,"file_translation":file_text,"metrics_requests":metrics.total.requests})).unwrap()).unwrap();
    }
    #[test]
    fn managed_commands_clear_python_overrides_and_keep_version_assertions_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let paths = StoragePaths::new(dir.path().into());
        let mut cmd = command("python");
        cmd.env("PYTHONHOME", "another Python")
            .env("PYTHONPATH", "foreign packages")
            .env("PYTHONOPTIMIZE", "2");
        configure(&mut cmd, &paths, dir.path()).unwrap();
        let env = cmd.get_envs().collect::<std::collections::HashMap<_, _>>();
        for key in [
            "PYTHONHOME",
            "PYTHONPATH",
            "PYTHONOPTIMIZE",
            "PYTHONINSPECT",
            "PYTHONSTARTUP",
        ] {
            assert_eq!(env[std::ffi::OsStr::new(key)], None);
        }
        assert_eq!(
            env[std::ffi::OsStr::new("PYTHONNOUSERSITE")],
            Some(std::ffi::OsStr::new("1"))
        );
    }
    #[test]
    fn selected_version_must_match_the_verified_installed_runtime() {
        let root = tempfile::tempdir().unwrap();
        let executable = python(root.path());
        fs::create_dir_all(executable.parent().unwrap()).unwrap();
        fs::write(executable, b"test interpreter").unwrap();
        fs::write(root.path().join("installed-version"), "1.9.5").unwrap();
        let mut instance = Instance::default();
        assert!(installed(root.path()));
        assert!(!selected_ready(root.path(), &instance));
        instance.server.libretranslate_version = "1.9.5".into();
        assert!(selected_ready(root.path(), &instance));
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "installs older releases into an isolated runtime and translates using copied local models"]
    fn selected_runtime_versions_install_and_translate() {
        let source = PathBuf::from(
            std::env::var_os("LIBRETRANSLATE_GUI_EXISTING_TEST_DATA_ROOT")
                .expect("Set existing runtime data root"),
        );
        let original = crate::model::load(&source)
            .unwrap()
            .into_iter()
            .find(|instance| instance.languages.contains("ro"))
            .unwrap();
        let base = std::env::current_dir().unwrap().join(".runtime-test");
        let root = base.join(format!("version-check-{}", uuid::Uuid::new_v4()));
        let mut paths = StoragePaths::new(root.clone());
        paths.cache = base.join("version-check-cache");
        paths.python = source.join("python");
        let mut instance = Instance {
            languages: "en,ro".into(),
            name: "Runtime version QA".into(),
            threads: 1,
            ..Default::default()
        };
        fs::create_dir_all(root.join("tools")).unwrap();
        let installer = format!("uv-{UV_VERSION}{}", std::env::consts::EXE_SUFFIX);
        fs::copy(
            source.join("tools").join(&installer),
            root.join("tools").join(installer),
        )
        .unwrap();
        fn copy_models(from: &Path, to: &Path) {
            fs::create_dir_all(to).unwrap();
            for entry in fs::read_dir(from).unwrap() {
                let entry = entry.unwrap();
                let destination = to.join(entry.file_name());
                if entry.file_type().unwrap().is_dir() {
                    copy_models(&entry.path(), &destination);
                } else if entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "json")
                    || fs::hard_link(entry.path(), &destination).is_err()
                {
                    fs::copy(entry.path(), destination).unwrap();
                }
            }
        }
        let dir = instance_dir(&root, instance.id);
        for model in ["translate-en_ro-1_9", "translate-ro_en-1_9"] {
            copy_models(
                &instance_dir(&source, original.id)
                    .join("packages")
                    .join(model),
                &dir.join("packages").join(model),
            );
        }
        let cancel = AtomicBool::new(false);
        let log: Logger = Arc::new(|line| println!("{line}"));
        for version in ["1.9.1", "1.9.5"] {
            instance.server.libretranslate_version = version.into();
            install(&paths, &instance, &cancel, &log).unwrap();
            assert_eq!(installed_version(&dir).as_deref(), Some(version));
            instance.port = {
                let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                socket.local_addr().unwrap().port()
            };
            let mut child = start(&paths, &instance, &log).unwrap();
            let client = Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(120);
            while !client
                .get(format!("{}/languages", instance.url()))
                .send()
                .is_ok_and(|response| response.status().is_success())
            {
                assert!(
                    child.0.try_wait().unwrap().is_none(),
                    "Server failed for {version}"
                );
                assert!(std::time::Instant::now() < deadline);
                thread::sleep(Duration::from_millis(200));
            }
            let result: serde_json::Value = client
                .post(format!("{}/translate", instance.url()))
                .json(&serde_json::json!({"q":"Hello","source":"en","target":"ro"}))
                .send()
                .unwrap()
                .error_for_status()
                .unwrap()
                .json()
                .unwrap();
            assert!(
                result["translatedText"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty() && text != "Hello")
            );
            println!("Verified LibreTranslate {version}: {result}");
            drop(child);
            assert!(wait_for_port_close(instance.port, Duration::from_secs(5)));
        }
        crate::model::save(&root, &[instance]).unwrap();
        fs::write(
            base.join("version-check-latest.txt"),
            root.to_string_lossy().as_bytes(),
        )
        .unwrap();
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "uses existing en/ro models and a temporary server to verify advanced options"]
    fn advanced_server_options_work_with_api_keys_metrics_and_prefix() {
        let source = PathBuf::from(
            std::env::var_os("LIBRETRANSLATE_GUI_EXISTING_TEST_DATA_ROOT")
                .expect("Set existing runtime data root"),
        );
        let instance = crate::model::load(&source)
            .unwrap()
            .into_iter()
            .find(|instance| instance.languages.contains("ro"))
            .unwrap();
        let original = instance_dir(&source, instance.id);
        let base = std::env::current_dir().unwrap().join(".runtime-test");
        let temporary = tempfile::tempdir_in(base).unwrap();
        let paths = StoragePaths::new(temporary.path().to_path_buf());
        let mut options = crate::model::ServerOptions {
            char_limit: Some(12),
            batch_limit: Some(2),
            request_limit: None,
            request_time_cost: Some(5),
            alternatives_limit: Some(1),
            frontend_title: "GUI test".into(),
            frontend_source: "en".into(),
            frontend_target: "ro".into(),
            frontend_timeout: 750,
            file_translation: false,
            api_keys: true,
            require_api_key: true,
            translation_cache: true,
            metrics: true,
            url_prefix: "/gui-test".into(),
            ..Default::default()
        };
        let mut validation = Instance {
            server: options.clone(),
            ..Default::default()
        };
        validation.server.api_key = "test-key".into();
        validation.validate(&[]).unwrap();
        options.api_key = "test-key".into();
        let port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let mut cmd = command(python(&original));
        configure(&mut cmd, &paths, temporary.path()).unwrap();
        cmd.env(
            "ARGOS_PACKAGES_DIR",
            dunce::simplified(&original).join("packages"),
        )
        .env("LIBRETRANSLATE_GUI_API_KEY", "test-key")
        .env(
            "LIBRETRANSLATE_GUI_API_DB",
            temporary.path().join("data/api_keys.db"),
        )
        .env("LIBRETRANSLATE_GUI_KEY_LIMIT", "-1")
        .env("LT_METRICS_AUTH_TOKEN", "test-metrics-token");
        cmd.arg("-c").arg(format!("{CUDA_BOOTSTRAP}\n{API_KEY_BOOTSTRAP}\nfrom libretranslate.main import main\nmain()"))
           .args(["--host","127.0.0.1","--port",&port.to_string(),"--load-only","en,ro","--threads","1","--api-keys-db-path"])
           .arg(temporary.path().join("data/api_keys.db")).args(options.args());
        let mut child = OwnedChild::spawn(&mut cmd).unwrap();
        let log: Logger = Arc::new(|line| println!("{line}"));
        pipe_logs(&mut child.0, &log);
        let client = Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap();
        let url = format!("http://127.0.0.1:{port}/gui-test");
        let deadline = std::time::Instant::now() + Duration::from_secs(90);
        loop {
            if client
                .get(format!("{url}/languages"))
                .send()
                .is_ok_and(|response| response.status().is_success())
            {
                break;
            }
            assert!(child.0.try_wait().unwrap().is_none());
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(Duration::from_millis(200));
        }
        let rejected = client
            .post(format!("{url}/translate"))
            .json(&serde_json::json!({"q":"Hello","source":"en","target":"ro"}))
            .send()
            .unwrap();
        assert!(!rejected.status().is_success());
        let translated: serde_json::Value = client
            .post(format!("{url}/translate"))
            .json(
                &serde_json::json!({"q":"Hello","source":"en","target":"ro","api_key":"test-key"}),
            )
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .unwrap();
        println!("Advanced configuration translation: {translated}");
        assert!(
            translated["translatedText"]
                .as_str()
                .is_some_and(|text| !text.is_empty() && text != "Hello")
        );
        assert!(!client.post(format!("{url}/translate")).json(&serde_json::json!({"q":"This is longer than twelve characters","source":"en","target":"ro","api_key":"test-key"})).send().unwrap().status().is_success());
        assert!(
            !client
                .get(format!("{url}/metrics"))
                .send()
                .unwrap()
                .status()
                .is_success()
        );
        assert!(
            client
                .get(format!("{url}/metrics"))
                .bearer_auth("test-metrics-token")
                .send()
                .unwrap()
                .status()
                .is_success()
        );
        drop(child);
        assert!(wait_for_port_close(port, Duration::from_secs(5)));
    }

    #[test]
    fn restores_interrupted_model_replacement_and_rejects_escape_paths() {
        let root = tempfile::tempdir().unwrap();
        let transaction = root.path().join("model-transactions/test");
        fs::create_dir_all(transaction.join("old/model")).unwrap();
        fs::create_dir_all(root.path().join("packages")).unwrap();
        fs::write(transaction.join("old/model/model.bin"), b"original").unwrap();
        fs::write(
            transaction.join("destination.json"),
            br#""translate-en_ro-1_9""#,
        )
        .unwrap();
        recover_model_transactions(root.path()).unwrap();
        assert_eq!(
            fs::read(
                root.path()
                    .join("packages/translate-en_ro-1_9/model/model.bin")
            )
            .unwrap(),
            b"original"
        );
        fs::write(transaction.join("destination.json"), br#""../outside""#).unwrap();
        assert!(recover_model_transactions(root.path()).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn python_model_paths_do_not_use_verbatim_windows_prefixes() {
        let temp = tempfile::tempdir().unwrap();
        let canonical = temp.path().canonicalize().unwrap();
        let paths = StoragePaths::new(canonical.clone());
        let dir = canonical.join("instance");
        let mut cmd = command("python");
        configure(&mut cmd, &paths, &dir).unwrap();
        let packages = cmd
            .get_envs()
            .find(|(key, _)| *key == "ARGOS_PACKAGES_DIR")
            .unwrap()
            .1
            .unwrap()
            .to_string_lossy();
        assert!(!packages.starts_with(r"\\?\"));
        assert!(std::path::Path::new(packages.as_ref()).is_absolute());
    }

    #[cfg(windows)]
    #[test]
    fn windows_job_stops_launcher_and_descendants() {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::*;
        let mut cmd = command("cmd.exe");
        cmd.args(["/C", "ping -n 30 127.0.0.1 >NUL"]);
        let child = OwnedChild::spawn(&mut cmd).unwrap();
        let job = child.1.try_clone().unwrap();
        let active = || {
            let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
            assert_ne!(
                unsafe {
                    QueryInformationJobObject(
                        job.as_raw_handle(),
                        JobObjectBasicAccountingInformation,
                        &mut info as *mut _ as *mut _,
                        std::mem::size_of_val(&info) as u32,
                        std::ptr::null_mut(),
                    )
                },
                0
            );
            info.ActiveProcesses
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while active() < 2 {
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(Duration::from_millis(20));
        }
        drop(child);
        assert_eq!(active(), 0, "The launcher's descendant escaped supervision");
    }
    #[test]
    fn runtime_uses_custom_python_and_download_folders() {
        let root = tempfile::tempdir().unwrap();
        let paths = StoragePaths {
            root: root.path().join("data"),
            cache: root.path().join("downloads"),
            python: root.path().join("runtimes"),
        };
        let dir = paths.root.join("instances/test");
        let mut cmd = command("python");
        configure(&mut cmd, &paths, &dir).unwrap();
        let env: std::collections::HashMap<_, _> = cmd
            .get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().to_string(),
                    v.map(|v| v.to_string_lossy().to_string()),
                )
            })
            .collect();
        assert_eq!(env["UV_CACHE_DIR"].as_deref(), paths.cache.to_str());
        assert_eq!(
            env["UV_PYTHON_INSTALL_DIR"].as_deref(),
            paths.python.to_str()
        );
        assert_eq!(env["PYTHONUTF8"].as_deref(), Some("1"));
        assert_eq!(
            env["ARGOS_PACKAGES_DIR"].as_deref(),
            dir.join("packages").to_str()
        );
    }
    #[test]
    fn rejects_tampered_and_malformed_downloads() {
        let checksum = format!("{:x}  uv.zip", Sha256::digest(b"valid"));
        assert!(verify_checksum(b"valid", &checksum).is_ok());
        assert!(verify_checksum(b"changed", &checksum).is_err());
        assert!(verify_checksum(b"valid", "").is_err());
    }
    #[test]
    fn install_marker_alone_is_not_a_runtime() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("installed-version"),
            LIBRETRANSLATE_VERSION,
        )
        .unwrap();
        assert!(!installed(root.path()));
    }
}
