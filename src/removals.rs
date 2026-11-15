//! Reversible instance deletion until the configuration change is committed.
use crate::paths::same_path;
use crate::{model::Instance, runtime};
use anyhow::{Context, Result, ensure};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::mpsc::{self, Sender},
};
use uuid::Uuid;

const DIRECTORY: &str = "instance-removals";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema: u8,
    instance: Uuid,
}

pub struct Removal {
    root: PathBuf,
    stage: PathBuf,
    id: Uuid,
    lease: Option<fs::File>,
}

fn directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_dir()
            && !metadata.file_type().is_symlink()
            && same_path(&dunce::canonicalize(path)?, path),
        "Instance removal contains a linked or invalid directory: {}",
        path.display()
    );
    Ok(())
}
fn parent(root: &Path) -> Result<PathBuf> {
    let path = root.join(DIRECTORY);
    match fs::create_dir(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    directory(&path)?;
    Ok(path)
}
fn finalization_lock(root: &Path) -> Result<fs::File> {
    let folder = parent(root)?;
    let path = folder.join("finalization.lock");
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "Invalid instance removal finalization lease"
        );
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.lock_exclusive()?;
    Ok(file)
}
fn lock(stage: &Path) -> Result<Option<fs::File>> {
    let path = stage.join("lease");
    if let Ok(metadata) = fs::symlink_metadata(&path) {
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "Invalid removal lease"
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
fn empty_stage(stage: &Path) -> Result<()> {
    // A crash while creating a journal cannot have moved the instance yet.
    for entry in fs::read_dir(stage)? {
        let entry = entry?;
        ensure!(
            matches!(entry.file_name().to_str(), Some("lease" | "journal.json"))
                && entry.file_type()?.is_file(),
            "Unrecognized files in incomplete instance removal: {}",
            stage.display()
        );
    }
    fs::remove_dir_all(stage)?;
    Ok(())
}
impl Removal {
    pub fn stage(root: &Path, id: Uuid) -> Result<Option<Self>> {
        let root = dunce::canonicalize(root)?;
        let source = runtime::instance_dir(&root, id);
        match fs::symlink_metadata(&source) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        directory(source.parent().unwrap())?;
        directory(&source)?;
        // Recovery must not see a half-created stage or its incomplete journal.
        let _finalizing = finalization_lock(&root)?;
        let stage = root.join(DIRECTORY).join(Uuid::new_v4().to_string());
        fs::create_dir(&stage)?;
        let result = (|| {
            let lease = lock(&stage)?.context("Instance removal is already active")?;
            let mut journal = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(stage.join("journal.json"))?;
            journal.write_all(&serde_json::to_vec(&Journal {
                schema: 1,
                instance: id,
            })?)?;
            journal.sync_all()?;
            drop(journal);
            fs::rename(&source, stage.join("instance"))?;
            Ok(Self {
                root,
                stage: stage.clone(),
                id,
                lease: Some(lease),
            })
        })();
        if result.is_err() {
            let _ = empty_stage(&stage);
        }
        result.map(Some)
    }
    fn validate(&self) -> Result<()> {
        ensure!(
            self.stage.parent() == Some(self.root.join(DIRECTORY).as_path()),
            "Invalid instance removal location"
        );
        directory(&self.root)?;
        directory(self.stage.parent().unwrap())?;
        directory(&self.stage)?;
        let journal = self.stage.join("journal.json");
        let metadata = fs::symlink_metadata(&journal)?;
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= 1024,
            "Invalid instance removal journal"
        );
        let journal: Journal = serde_json::from_slice(&fs::read(journal)?)?;
        ensure!(
            journal.schema == 1 && journal.instance == self.id,
            "Invalid instance removal identity"
        );
        Ok(())
    }
    pub fn rollback(mut self) -> Result<()> {
        let _finalizing = finalization_lock(&self.root)?;
        self.rollback_locked()
    }
    fn rollback_locked(&mut self) -> Result<()> {
        self.validate()?;
        let staged = self.stage.join("instance");
        if staged.try_exists()? {
            directory(&staged)?;
            let original = runtime::instance_dir(&self.root, self.id);
            directory(original.parent().unwrap())?;
            ensure!(
                !original.try_exists()?,
                "Cannot restore removed instance because its original folder is occupied"
            );
            fs::rename(staged, original)
                .context("Could not restore the instance after a configuration save failure")?;
        }
        self.lease.take();
        empty_stage(&self.stage)
    }
    pub fn finish(mut self) -> Result<()> {
        self.validate()?;
        let staged = self.stage.join("instance");
        if staged.try_exists()? {
            directory(&staged)?;
            fs::remove_dir_all(staged)?;
        }
        // Keep scanning recovery out of the close-lease/delete-metadata gap.
        // The payload deletion above can be large and deliberately holds no
        // parent-level lock, so reopening never waits for bulk cleanup.
        let _finalizing = finalization_lock(&self.root)?;
        self.lease.take();
        empty_stage(&self.stage)
    }
}
pub fn recover(root: &Path, instances: &[Instance]) -> Result<Vec<Removal>> {
    let root = dunce::canonicalize(root)?;
    let folder = root.join(DIRECTORY);
    if !folder.try_exists()? {
        return Ok(Vec::new());
    }
    directory(&folder)?;
    let _finalizing = finalization_lock(&root)?;
    let mut committed = Vec::new();
    for entry in fs::read_dir(&folder)? {
        let entry = entry?;
        let name = entry.file_name();
        if name
            .to_str()
            .and_then(|name| Uuid::parse_str(name).ok())
            .is_none()
        {
            continue;
        }
        let stage = entry.path();
        match recover_one(&root, &stage, instances) {
            Ok(Some(removal)) => committed.push(removal),
            Ok(None) => {}
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound)
                    && !stage.join("instance").try_exists()? => {}
            Err(error) => return Err(error),
        }
    }
    Ok(committed)
}
fn recover_one(root: &Path, stage: &Path, instances: &[Instance]) -> Result<Option<Removal>> {
    directory(stage)?;
    let Some(lease) = lock(stage)? else {
        return Ok(None);
    };
    let journal = stage.join("journal.json");
    if !journal.try_exists()? {
        drop(lease);
        empty_stage(stage)?;
        return Ok(None);
    }
    let metadata = fs::symlink_metadata(&journal)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= 1024,
        "Invalid instance removal journal"
    );
    let journal: Journal = match serde_json::from_slice(&fs::read(journal)?) {
        Ok(journal) => journal,
        Err(_) if !stage.join("instance").try_exists()? => {
            // Journal writes precede the payload rename. A torn journal with
            // no staged payload leaves the original instance untouched.
            drop(lease);
            empty_stage(stage)?;
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    ensure!(journal.schema == 1, "Unsupported instance removal journal");
    let mut removal = Removal {
        root: root.into(),
        stage: stage.into(),
        id: journal.instance,
        lease: Some(lease),
    };
    if instances
        .iter()
        .any(|instance| instance.id == journal.instance)
    {
        removal.rollback_locked()?;
        Ok(None)
    } else {
        Ok(Some(removal))
    }
}
pub fn cleaner(
    initial: Vec<Removal>,
    report: impl Fn(String) + Send + 'static,
) -> Result<Sender<Removal>> {
    let (send, receive) = mpsc::channel::<Removal>();
    std::thread::Builder::new().name("instance-file-cleanup".into()).spawn(move|| {
        for removal in initial.into_iter().chain(receive) {
            if let Err(error)=removal.finish() {report(format!("The instance was removed, but some files remain in instance-removals. Cleanup will retry on restart. {error:#}"));}
        }
    })?;
    Ok(send)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn instance(root: &Path) -> Instance {
        let instance = Instance::default();
        let dir = runtime::instance_dir(root, instance.id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("model.bin"), b"model").unwrap();
        instance
    }
    #[test]
    fn interrupted_uncommitted_removal_restores_files_and_committed_removal_cleans_them() {
        let dir = tempfile::tempdir().unwrap();
        let instance = instance(dir.path());
        let removal = Removal::stage(dir.path(), instance.id).unwrap().unwrap();
        assert!(!runtime::instance_dir(dir.path(), instance.id).exists());
        drop(removal);
        assert!(
            recover(dir.path(), std::slice::from_ref(&instance))
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            fs::read(runtime::instance_dir(dir.path(), instance.id).join("model.bin")).unwrap(),
            b"model"
        );
        drop(Removal::stage(dir.path(), instance.id).unwrap());
        let mut recovered = recover(dir.path(), &[]).unwrap();
        assert_eq!(recovered.len(), 1);
        recovered.pop().unwrap().finish().unwrap();
        assert_eq!(stages(dir.path()), 0);
    }
    #[test]
    fn active_cleanup_is_not_adopted_and_conflicting_restore_preserves_both_copies() {
        let dir = tempfile::tempdir().unwrap();
        let instance = instance(dir.path());
        let removal = Removal::stage(dir.path(), instance.id).unwrap().unwrap();
        assert!(recover(dir.path(), &[]).unwrap().is_empty());
        let original = runtime::instance_dir(dir.path(), instance.id);
        fs::create_dir_all(&original).unwrap();
        fs::write(original.join("new.txt"), b"new").unwrap();
        let staged = removal.stage.clone();
        drop(removal);
        assert!(recover(dir.path(), &[instance]).is_err());
        assert!(original.join("new.txt").is_file());
        assert!(staged.join("instance/model.bin").is_file());
    }
    #[test]
    fn malformed_journals_never_delete_instance_data() {
        let dir = tempfile::tempdir().unwrap();
        let instance = instance(dir.path());
        let removal = Removal::stage(dir.path(), instance.id).unwrap().unwrap();
        let stage = removal.stage.clone();
        drop(removal);
        fs::write(stage.join("journal.json"), b"{}").unwrap();
        assert!(recover(dir.path(), &[]).is_err());
        assert!(stage.join("instance/model.bin").is_file());
    }
    fn stages(root: &Path) -> usize {
        fs::read_dir(root.join(DIRECTORY))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().unwrap().is_dir())
            .count()
    }
    #[test]
    fn torn_journal_before_rename_does_not_block_startup_or_touch_original() {
        let dir = tempfile::tempdir().unwrap();
        let instance = instance(dir.path());
        let stage = parent(&dunce::canonicalize(dir.path()).unwrap())
            .unwrap()
            .join(Uuid::new_v4().to_string());
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("journal.json"), b"{\"schema\":1,").unwrap();
        assert!(
            recover(dir.path(), std::slice::from_ref(&instance))
                .unwrap()
                .is_empty()
        );
        assert!(!stage.exists());
        assert!(
            runtime::instance_dir(dir.path(), instance.id)
                .join("model.bin")
                .is_file()
        );
    }
    #[test]
    fn cleanup_and_reopen_can_overlap_without_delete_pending_errors() {
        let dir = tempfile::tempdir().unwrap();
        let mut removals = Vec::new();
        for _ in 0..80 {
            let instance = instance(dir.path());
            removals.push(Removal::stage(dir.path(), instance.id).unwrap().unwrap());
        }
        let (errors, receive) = mpsc::channel();
        let worker = cleaner(removals, move |error| {
            let _ = errors.send(error);
        })
        .unwrap();
        drop(worker);
        let start = std::time::Instant::now();
        let mut checks = 0;
        loop {
            assert!(recover(dir.path(), &[]).unwrap().is_empty());
            checks += 1;
            if stages(dir.path()) == 0 && checks >= 50 {
                break;
            }
            assert!(start.elapsed() < std::time::Duration::from_secs(10));
            std::thread::yield_now();
        }
        assert!(receive.try_iter().next().is_none());
    }
    #[cfg(windows)]
    #[test]
    fn windows_case_aliases_match_normal_directory_names() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("Instances")).unwrap();
        fs::create_dir(dir.path().join("INSTANCE-REMOVALS")).unwrap();
        let instance = instance(dir.path());
        let removal = Removal::stage(dir.path(), instance.id).unwrap().unwrap();
        removal.rollback().unwrap();
        assert!(
            runtime::instance_dir(dir.path(), instance.id)
                .join("model.bin")
                .is_file()
        );
    }
    #[cfg(windows)]
    #[test]
    fn locked_file_cleanup_preserves_journal_and_retries_after_release() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = tempfile::tempdir().unwrap();
        let instance = instance(dir.path());
        let removal = Removal::stage(dir.path(), instance.id).unwrap().unwrap();
        let stage = removal.stage.clone();
        let locked = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(stage.join("instance/model.bin"))
            .unwrap();
        assert!(removal.finish().is_err());
        assert!(stage.join("journal.json").is_file());
        drop(locked);
        let mut retry = recover(dir.path(), &[]).unwrap();
        assert_eq!(retry.len(), 1);
        retry.pop().unwrap().finish().unwrap();
        assert!(!stage.exists());
    }
    #[cfg(any(unix, windows))]
    fn link(source: &Path, target: &Path) -> bool {
        #[cfg(unix)]
        let result = std::os::unix::fs::symlink(source, target);
        #[cfg(windows)]
        let result = std::os::windows::fs::symlink_dir(source, target);
        match result {
            Ok(()) => true,
            Err(error)
                if error.kind() == std::io::ErrorKind::PermissionDenied
                    || error.kind() == std::io::ErrorKind::Unsupported =>
            {
                eprintln!("Directory link test skipped: {error}");
                false
            }
            Err(error) => panic!("Could not create test directory link: {error}"),
        }
    }
    #[cfg(any(unix, windows))]
    #[test]
    #[cfg_attr(
        windows,
        ignore = "requires Windows directory-symlink privilege; enabled by default on Unix"
    )]
    fn linked_instance_parent_is_rejected_and_nested_links_leave_external_files_intact() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let instance = Instance::default();
        let external = outside.path().join(instance.id.to_string());
        fs::create_dir(&external).unwrap();
        fs::write(external.join("keep"), b"keep").unwrap();
        if !link(outside.path(), &dir.path().join("instances")) {
            return;
        }
        assert!(Removal::stage(dir.path(), instance.id).is_err());
        assert!(external.join("keep").is_file());
        #[cfg(unix)]
        fs::remove_file(dir.path().join("instances")).unwrap();
        #[cfg(windows)]
        fs::remove_dir(dir.path().join("instances")).unwrap();
        let own = super::tests::instance(dir.path());
        assert!(link(
            outside.path(),
            &runtime::instance_dir(dir.path(), own.id).join("linked-models")
        ));
        Removal::stage(dir.path(), own.id)
            .unwrap()
            .unwrap()
            .finish()
            .unwrap();
        assert_eq!(fs::read(external.join("keep")).unwrap(), b"keep");
    }
}
