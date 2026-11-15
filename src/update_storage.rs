//! Reclaim owned, inactive update stages while keeping the latest rollback copy.
use super::{Job, create_parent, validate_job};
use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const ORPHAN_AGE: u64 = 24 * 60 * 60;
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Phase {
    Preparing,
    Ready,
    Applied,
    Failed,
    Discarding,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Info {
    schema: u8,
    created: u64,
    finished: Option<u64>,
    phase: Phase,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && same_path(&dunce::canonicalize(path)?, path),
        "Invalid update storage directory"
    );
    Ok(())
}
pub(super) use crate::paths::same_path;
fn owned(root: &Path, stage: &Path) -> Result<()> {
    directory(root)?;
    directory(&root.join(".updates"))?;
    directory(stage)?;
    ensure!(
        stage
            .parent()
            .is_some_and(|parent| same_path(parent, &root.join(".updates")))
            && stage
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| uuid::Uuid::parse_str(name).ok())
                .is_some(),
        "Invalid update storage location"
    );
    Ok(())
}
pub(super) fn lease(stage: &Path) -> Result<Option<fs::File>> {
    let path = stage.join("lease");
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "Invalid update stage lease"
        );
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(Some(file)),
        Err(error)
            if error.kind() == std::io::ErrorKind::WouldBlock
                || error.raw_os_error() == fs2::lock_contended_error().raw_os_error() =>
        {
            Ok(None)
        }
        Err(error) => Err(error.into()),
    }
}
fn read_info(stage: &Path) -> Result<Info> {
    let path = stage.join("stage.json");
    let metadata = fs::symlink_metadata(&path)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= 4096,
        "Invalid update stage metadata"
    );
    let info: Info = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(info.schema == 1, "Unsupported update stage metadata");
    Ok(info)
}
fn write_info(stage: &Path, info: &Info) -> Result<()> {
    let temporary = stage.join(format!("stage-{}.json.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&serde_json::to_vec(info)?)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, stage.join("stage.json"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
fn tree(path: &Path) -> Result<()> {
    let mut folders = vec![path.to_path_buf()];
    let mut entries = 0;
    while let Some(folder) = folders.pop() {
        directory(&folder)?;
        for entry in fs::read_dir(folder)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            entries += 1;
            ensure!(
                entries <= 50000 && !kind.is_symlink() && (kind.is_file() || kind.is_dir()),
                "Unexpected files in update storage"
            );
            if kind.is_dir() {
                folders.push(entry.path());
            }
        }
    }
    Ok(())
}
fn discard(root: &Path, stage: &Path) -> Result<()> {
    owned(root, stage)?;
    tree(stage)?;
    let mut info = read_info(stage).unwrap_or(Info {
        schema: 1,
        created: now(),
        finished: None,
        phase: Phase::Preparing,
    });
    if info.phase != Phase::Discarding {
        // Record the decision before removing any payload. A partially deleted
        // backup must not later be mistaken for a complete rollback candidate.
        info.phase = Phase::Discarding;
        info.finished = Some(now());
        write_info(stage, &info)?;
    }
    for entry in fs::read_dir(stage)? {
        let entry = entry?;
        let name = entry.file_name();
        if ["stage.json", "job.json", "lease"]
            .iter()
            .any(|control| same_path(Path::new(&name), Path::new(control)))
        {
            continue;
        }
        let path = entry.path();
        let kind = entry.file_type()?;
        ensure!(
            !kind.is_symlink(),
            "Linked update payload cannot be discarded"
        );
        if kind.is_dir() {
            directory(&path)?;
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(path)?;
        }
    }
    // Keep ownership and retry state until every potentially locked payload is
    // gone. Whole-stage remove_dir_all can otherwise delete metadata first.
    let result = (|| -> Result<()> {
        for control in ["job.json", "lease", "stage.json"] {
            match fs::remove_file(stage.join(control)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        match fs::remove_dir(stage) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    })();
    if result.is_err()
        && stage.try_exists().unwrap_or(false)
        && !stage.join("stage.json").try_exists().unwrap_or(true)
    {
        // A directory handle can still deny the final unlink after its files
        // were removed. Restore retry metadata while that directory remains.
        let _ = write_info(stage, &info);
    }
    result
}

pub(super) struct Preparation {
    root: PathBuf,
    stage: PathBuf,
    lease: Option<fs::File>,
    keep: bool,
}
impl Preparation {
    pub(super) fn new(root: &Path) -> Result<Self> {
        create_parent(root, Path::new(".updates"))?;
        let stage = root.join(".updates").join(uuid::Uuid::new_v4().to_string());
        fs::create_dir(&stage)?;
        let stage = dunce::canonicalize(stage)?;
        let mut preparing = Self {
            root: root.into(),
            stage,
            lease: None,
            keep: false,
        };
        preparing.lease =
            Some(lease(&preparing.stage)?.context("Update preparation is already active")?);
        write_info(
            &preparing.stage,
            &Info {
                schema: 1,
                created: now(),
                finished: None,
                phase: Phase::Preparing,
            },
        )?;
        Ok(preparing)
    }
    pub(super) fn path(&self) -> &Path {
        &self.stage
    }
    pub(super) fn ready(&mut self) -> Result<()> {
        let mut info = read_info(&self.stage)?;
        info.phase = Phase::Ready;
        write_info(&self.stage, &info)?;
        self.keep = true;
        Ok(())
    }
}
impl Drop for Preparation {
    fn drop(&mut self) {
        if !self.keep {
            let _ = discard(&self.root, &self.stage);
        }
        self.lease.take();
    }
}
pub(super) fn finished(job: &Job, success: bool) -> Result<()> {
    // Legacy helpers have no stage metadata; adopt only the already validated job.
    validate_job(job)?;
    let mut info = read_info(&job.staging).unwrap_or(Info {
        schema: 1,
        created: now(),
        finished: None,
        phase: Phase::Ready,
    });
    info.phase = if success {
        Phase::Applied
    } else {
        Phase::Failed
    };
    info.finished = Some(now());
    write_info(&job.staging, &info)
}
pub(super) fn discard_ready(job: &Job) -> Result<()> {
    validate_job(job)?;
    let Some(guard) = lease(&job.staging)? else {
        return Ok(());
    };
    ensure!(
        read_info(&job.staging)?.phase == Phase::Ready
            && !job.staging.join("backup").try_exists()?,
        "Update stage needs recovery before cleanup"
    );
    let result = discard(&job.root, &job.staging);
    drop(guard);
    result
}
fn has_backup(stage: &Path) -> Result<bool> {
    let backup = stage.join("backup");
    if !backup.try_exists()? {
        return Ok(false);
    }
    directory(&backup)?;
    let mut folders = vec![backup];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(folder)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            ensure!(!kind.is_symlink(), "Invalid update backup");
            if kind.is_file() {
                return Ok(true);
            }
            if kind.is_dir() {
                folders.push(entry.path());
            }
        }
    }
    Ok(false)
}
fn read_job(root: &Path, stage: &Path) -> Option<Job> {
    let path = stage.join("job.json");
    let metadata = fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() >= 128 * 1024 {
        return None;
    }
    let job: Job = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    (same_path(&job.root, root) && same_path(&job.staging, stage) && validate_job(&job).is_ok())
        .then_some(job)
}
fn defer(error: anyhow::Error, deferred: &mut Option<anyhow::Error>) {
    if error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    {
        return;
    }
    if deferred.is_none() {
        *deferred = Some(error);
    }
}
fn trim_retained(stage: &Path) -> Result<()> {
    tree(stage)?;
    for name in [
        "release.archive",
        "release.msi",
        "bundle",
        "helper.exe",
        "helper",
    ] {
        let path = stage.join(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path)?,
            Ok(_) => fs::remove_file(path)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
pub(super) fn maintain(root: &Path) -> Result<()> {
    let root = dunce::canonicalize(root)?;
    let updates = root.join(".updates");
    if !updates.try_exists()? {
        return Ok(());
    }
    directory(&updates)?;
    let Some(_maintenance) = lease(&updates)? else {
        return Ok(());
    };
    maintain_entries(&root, fs::read_dir(&updates)?)
}
fn maintain_entries(
    root: &Path,
    entries: impl Iterator<Item = std::io::Result<fs::DirEntry>>,
) -> Result<()> {
    let mut deferred = None;
    let mut stages = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                defer(error.into(), &mut deferred);
                continue;
            }
        };
        let stage = entry.path();
        if entry
            .file_name()
            .to_str()
            .and_then(|name| uuid::Uuid::parse_str(name).ok())
            .is_none()
        {
            continue;
        }
        // Unknown/linked or concurrently removed directories are never adopted.
        if owned(root, &stage).is_err() {
            continue;
        }
        let guard = match lease(&stage) {
            Ok(Some(guard)) => guard,
            Ok(None) => continue,
            Err(error)
                if error.downcast_ref::<std::io::Error>().is_some_and(|error| {
                    matches!(
                        error.kind(),
                        std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
                    )
                }) =>
            {
                continue;
            }
            Err(error) => {
                defer(error, &mut deferred);
                continue;
            }
        };
        match super::read_journal(&stage) {
            Ok(Some(journal)) if !journal.complete => continue,
            Err(error) => {
                defer(error, &mut deferred);
                continue;
            }
            _ => {}
        }
        let info = match read_info(&stage) {
            Ok(info) => info,
            Err(_) => {
                let Some(job) = read_job(root, &stage) else {
                    continue;
                };
                if super::parent_alive(job.parent) {
                    continue;
                }
                // Legacy jobs cannot prove install completion. Keep any backup.
                let result = (|| -> Result<()> {
                    tree(&stage)?;
                    if has_backup(&stage)? {
                        return Ok(());
                    }
                    let old = fs::metadata(&stage)?
                        .modified()?
                        .elapsed()
                        .is_ok_and(|age| age >= Duration::from_secs(7 * ORPHAN_AGE));
                    if old {
                        discard(root, &stage)?;
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    defer(error, &mut deferred);
                }
                drop(guard);
                continue;
            }
        };
        // A suspended app may resume between prepare and helper launch. Its
        // valid pending job remains owned even after the orphan-age threshold.
        if matches!(info.phase, Phase::Preparing | Phase::Ready)
            && read_job(root, &stage).is_some_and(|job| super::parent_alive(job.parent))
        {
            continue;
        }
        // Validate the entire stage before counting a backup as retainable.
        if let Err(error) = tree(&stage) {
            defer(error, &mut deferred);
            continue;
        }
        let backup = match has_backup(&stage) {
            Ok(backup) => backup,
            Err(error) => {
                defer(error, &mut deferred);
                continue;
            }
        };
        if info.phase == Phase::Applied {
            stages.push((stage, info, guard, backup));
        } else if info.phase == Phase::Discarding
            || (!backup
                && (info.phase == Phase::Failed
                    || now().saturating_sub(info.created) >= ORPHAN_AGE))
        {
            if let Err(error) = discard(root, &stage) {
                defer(error, &mut deferred);
            }
            drop(guard);
        }
    }
    stages
        .sort_by_key(|(stage, info, _, _)| (info.finished.unwrap_or(info.created), stage.clone()));
    let keep = stages.iter().rposition(|(_, _, _, backup)| *backup);
    for (index, (stage, _, guard, _)) in stages.into_iter().enumerate() {
        let result = if Some(index) == keep {
            trim_retained(&stage)
        } else {
            discard(root, &stage)
        };
        // Keep the stage lease through all path validation and removal. A helper
        // cannot adopt the same directory after validation but before deletion.
        drop(guard);
        if let Err(error) = result {
            defer(error, &mut deferred);
        }
    }
    deferred.map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ready_job(root: &Path, created: u64, parent: u32) -> Job {
        let mut preparation = Preparation::new(root).unwrap();
        let staging = preparation.path().to_path_buf();
        let job = Job {
            root: root.into(),
            staging: staging.clone(),
            executable: super::super::executable_name().into(),
            parent,
            version: "v0.3.0".into(),
            files: vec![super::super::executable_name().into()],
            msi: false,
        };
        fs::write(staging.join("job.json"), serde_json::to_vec(&job).unwrap()).unwrap();
        preparation.ready().unwrap();
        drop(preparation);
        write_info(
            &staging,
            &Info {
                schema: 1,
                created,
                finished: None,
                phase: Phase::Ready,
            },
        )
        .unwrap();
        job
    }
    #[test]
    fn suspended_live_parent_keeps_its_old_ready_job() {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        let job = ready_job(&root, 0, std::process::id());
        maintain(&root).unwrap();
        assert!(job.staging.join("job.json").is_file());
    }
    #[test]
    fn invalid_stage_does_not_block_cleanup_of_other_completed_stages() {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        let old = applied(&root, 1);
        let latest = applied(&root, 2);
        let invalid = ready_job(&root, 0, 0);
        fs::write(invalid.staging.join("backup"), b"not a directory; preserve").unwrap();
        assert!(maintain(&root).is_err());
        assert!(!old.exists());
        assert!(latest.join("backup/old.exe").is_file());
        assert!(invalid.staging.join("backup").is_file());
    }
    #[test]
    fn failed_metadata_replace_cleans_its_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        let stage = Preparation::new(&root).unwrap();
        fs::remove_file(stage.path().join("stage.json")).unwrap();
        fs::create_dir(stage.path().join("stage.json")).unwrap();
        let info = Info {
            schema: 1,
            created: 1,
            finished: None,
            phase: Phase::Ready,
        };
        assert!(write_info(stage.path(), &info).is_err());
        assert!(!fs::read_dir(stage.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".json.tmp")
        }));
    }
    #[test]
    fn stale_metadata_temporary_file_does_not_block_ready_transition() {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        let mut stage = Preparation::new(&root).unwrap();
        fs::write(
            stage.path().join("stage.json.tmp"),
            b"interrupted old metadata write",
        )
        .unwrap();
        stage.ready().unwrap();
        assert!(read_info(stage.path()).unwrap().phase == Phase::Ready);
    }
    #[test]
    fn stage_removed_after_inventory_does_not_abort_other_retention() {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        let old = applied(&root, 1);
        let latest = applied(&root, 2);
        let disappearing = Preparation::new(&root).unwrap();
        let entries = fs::read_dir(root.join(".updates"))
            .unwrap()
            .collect::<Vec<_>>();
        drop(disappearing);
        maintain_entries(&root, entries.into_iter()).unwrap();
        assert!(!old.exists());
        assert!(latest.join("backup/old.exe").is_file());
    }
    #[cfg(windows)]
    #[test]
    fn windows_path_components_accept_separator_aliases_without_parent_or_device_aliases() {
        assert!(same_path(
            Path::new(r"C:\App\.updates\stage"),
            Path::new("c:/app/.UPDATES/stage")
        ));
        assert!(same_path(
            Path::new(r"\\server\share\App\.updates"),
            Path::new("//SERVER/SHARE/app/.updates")
        ));
        assert!(!same_path(
            Path::new(r"C:\App\.updates"),
            Path::new(r"C:\App\other\..\.updates")
        ));
        assert!(!same_path(
            Path::new(r"C:\App\.updates"),
            Path::new(r"\\?\C:\App\.updates")
        ));
        assert!(!same_path(
            Path::new(r"C:\App\.updates"),
            Path::new(r"C:\Other\.updates")
        ));
    }
    #[cfg(windows)]
    #[test]
    fn existing_updates_folder_case_is_not_treated_as_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        fs::create_dir(root.join(".Updates")).unwrap();
        let stage = Preparation::new(&root).unwrap();
        let actual = dunce::canonicalize(stage.path()).unwrap();
        assert_eq!(stage.path(), actual);
        owned(&root, stage.path()).unwrap();
        maintain(&root).unwrap();
    }
    #[test]
    fn stage_can_be_reclaimed_without_releasing_its_cleanup_lease() {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        let job = ready_job(&root, 0, 0);
        let guard = lease(&job.staging).unwrap().unwrap();
        discard(&root, &job.staging).unwrap();
        assert!(!job.staging.exists());
        drop(guard);
    }
    #[cfg(windows)]
    #[test]
    fn locked_payload_keeps_cleanup_metadata_until_retry_succeeds() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        let job = ready_job(&root, 0, 0);
        let payload = job.staging.join("zz-payload");
        fs::create_dir(&payload).unwrap();
        let helper = payload.join("helper.exe");
        fs::write(&helper, b"locked helper image").unwrap();
        let guard = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&helper)
            .unwrap();
        assert!(maintain(&root).is_err());
        assert!(
            job.staging.join("stage.json").is_file(),
            "partial cleanup lost its retry metadata"
        );
        drop(guard);
        maintain(&root).unwrap();
        assert!(!job.staging.exists());
    }
    #[cfg(windows)]
    #[test]
    fn locked_stage_directory_restores_metadata_for_the_next_cleanup() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        let job = ready_job(&root, 0, 0);
        let guard = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .custom_flags(0x02000000)
            .open(&job.staging)
            .unwrap();
        assert!(maintain(&root).is_err());
        assert!(read_info(&job.staging).unwrap().phase == Phase::Discarding);
        drop(guard);
        maintain(&root).unwrap();
        assert!(!job.staging.exists());
    }
    #[cfg(windows)]
    #[test]
    fn partially_discarded_backup_is_not_retained_as_the_latest_rollback_copy() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        let old = applied(&root, 1);
        let latest = applied(&root, 2);
        let payload = old.join("zz-payload");
        fs::create_dir(&payload).unwrap();
        let helper = payload.join("helper.exe");
        fs::write(&helper, b"locked helper").unwrap();
        let guard = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(helper)
            .unwrap();
        assert!(maintain(&root).is_err());
        assert!(read_info(&old).unwrap().phase == Phase::Discarding);
        assert!(latest.join("backup/old.exe").is_file());
        drop(guard);
        maintain(&root).unwrap();
        assert!(!old.exists());
        assert!(latest.join("backup/old.exe").is_file());
    }
    fn applied(root: &Path, time: u64) -> PathBuf {
        let mut stage = Preparation::new(root).unwrap();
        let path = stage.path().to_path_buf();
        stage.ready().unwrap();
        drop(stage);
        fs::create_dir_all(path.join("backup")).unwrap();
        fs::write(path.join("backup/old.exe"), b"old").unwrap();
        fs::write(path.join("release.archive"), b"large archive").unwrap();
        write_info(
            &path,
            &Info {
                schema: 1,
                created: time,
                finished: Some(time),
                phase: Phase::Applied,
            },
        )
        .unwrap();
        path
    }
    #[test]
    fn failed_preparation_reclaims_partial_downloads() {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        let stage = Preparation::new(&root).unwrap();
        let path = stage.path().to_path_buf();
        fs::write(path.join("release.archive"), b"partial").unwrap();
        drop(stage);
        assert!(!path.exists());
    }
    #[test]
    fn retains_latest_backup_and_skips_active_and_unknown_stages() {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        let old = applied(&root, 1);
        let latest = applied(&root, 2);
        let active = Preparation::new(&root).unwrap();
        let unknown = root.join(".updates").join(uuid::Uuid::new_v4().to_string());
        fs::create_dir(&unknown).unwrap();
        fs::write(unknown.join("personal.txt"), b"keep").unwrap();
        maintain(&root).unwrap();
        assert!(!old.exists());
        assert!(latest.join("backup/old.exe").is_file());
        assert!(!latest.join("release.archive").exists());
        assert!(active.path().exists());
        assert!(unknown.join("personal.txt").is_file());
    }
    #[test]
    fn old_orphans_are_removed_but_failed_backups_survive() {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        let mut orphan = Preparation::new(&root).unwrap();
        let old = orphan.path().to_path_buf();
        orphan.ready().unwrap();
        drop(orphan);
        write_info(
            &old,
            &Info {
                schema: 1,
                created: 0,
                finished: None,
                phase: Phase::Ready,
            },
        )
        .unwrap();
        let failed = applied(&root, 1);
        write_info(
            &failed,
            &Info {
                schema: 1,
                created: 0,
                finished: Some(1),
                phase: Phase::Failed,
            },
        )
        .unwrap();
        maintain(&root).unwrap();
        assert!(!old.exists());
        assert!(failed.join("backup/old.exe").is_file());
    }
    #[cfg(unix)]
    #[test]
    fn linked_stages_cannot_remove_files_outside_the_app() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        let _stage = Preparation::new(&root).unwrap();
        fs::write(outside.path().join("keep"), b"keep").unwrap();
        std::os::unix::fs::symlink(
            outside.path(),
            root.join(".updates").join(uuid::Uuid::new_v4().to_string()),
        )
        .unwrap();
        maintain(&root).unwrap();
        assert!(outside.path().join("keep").is_file());
    }
}
