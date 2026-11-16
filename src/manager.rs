use crate::{
    downloads::{self, ModelDownload, SharedDownloads},
    model::{self, Instance},
    runtime::{self, Logger, OwnedChild},
    settings::StoragePaths,
};
use anyhow::{Context, Result, bail, ensure};
use fs2::FileExt;
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    RemoteChecking,
    RemoteOnline,
    RemoteOffline,
    NotInstalled,
    Installing,
    UpdatingModels,
    Stopped,
    Starting,
    Running,
    Unresponsive,
    Failed,
}
impl Status {
    pub fn can_translate(self) -> bool {
        matches!(self, Self::Running | Self::RemoteOnline)
    }
    pub fn active(self) -> bool {
        matches!(self, Self::Starting | Self::Running | Self::Unresponsive)
    }
    pub fn busy(self) -> bool {
        self.active() || matches!(self, Self::Installing | Self::UpdatingModels)
    }
}

#[derive(Clone, PartialEq)]
pub struct View {
    pub instance: Instance,
    pub status: Status,
    pub pid: Option<u32>,
}
pub enum Action {
    Save(Box<Instance>),
    Flush(Vec<Instance>, Sender<std::result::Result<(), String>>),
    Install(Uuid),
    UpdateModels(Uuid),
    CleanStorage(Uuid),
    Start(Uuid),
    Stop(Uuid),
    Remove(Uuid),
    CheckConnection(Uuid),
    Shutdown,
}
pub enum Event {
    Snapshot(Vec<View>),
    Error(String),
    SaveFailed { id: Uuid, details: String },
    Notice(Notice),
    Languages(Uuid, String, String),
}
pub enum Notice {
    Saved(Uuid),
    Removed,
    ModelsUpdated,
    StorageCleaned,
}
pub struct Log {
    pub id: Uuid,
    pub text: String,
}
enum Internal {
    RemoteHealth(Uuid, String, bool),
    Languages(Uuid, String, String),
    Installed(Uuid, std::result::Result<(), String>),
    Health(Uuid, u32, bool),
    ModelsUpdated(Uuid, std::result::Result<(), String>),
    StorageCleaned(Uuid, std::result::Result<(), String>),
}

pub struct Manager {
    pub actions: Sender<Action>,
    pub events: Receiver<Event>,
    pub logs: Receiver<Log>,
    pub downloads: SharedDownloads,
    #[cfg(test)]
    pub root: PathBuf,
    cancel: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Manager {
    #[cfg(test)]
    pub fn new(root: PathBuf) -> Result<Self> {
        Self::with_paths(StoragePaths::new(root))
    }
    pub fn with_paths(mut paths: StoragePaths) -> Result<Self> {
        let root = &paths.root;
        fs::create_dir_all(root)?;
        let root = dunce::canonicalize(root)?;
        fs::create_dir_all(&paths.cache)?;
        fs::create_dir_all(&paths.python)?;
        paths.root = root.clone();
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("manager.lock"))?;
        lock.try_lock_exclusive().context(
            "Another LibreTranslate Desktop window is using this data directory. Close it first.",
        )?;
        let instances = model::load_initialized(&root)?;
        let removals = crate::removals::recover(&root, &instances)?;
        let (actions, receive) = mpsc::channel();
        // Control acknowledgements must survive a temporarily busy renderer.
        // Repeated idle snapshots are coalesced by the actor below.
        let (send, events) = mpsc::channel();
        let cleanup_events = send.clone();
        let cleanup = crate::removals::cleaner(removals, move |details| {
            let _ = cleanup_events.send(Event::Error(details));
        })?;
        let (log_send, logs) = mpsc::sync_channel(1024);
        let cancel = Arc::new(AtomicBool::new(false));
        let thread_cancel = cancel.clone();
        let downloads = SharedDownloads::default();
        let thread_downloads = downloads.clone();
        let worker = thread::spawn(move || {
            Actor::new(
                paths,
                instances,
                send,
                log_send,
                thread_cancel,
                lock,
                thread_downloads,
            )
            .run(receive, cleanup)
        });
        Ok(Self {
            actions,
            events,
            logs,
            downloads,
            #[cfg(test)]
            root,
            cancel,
            worker: Some(worker),
        })
    }
}
impl Drop for Manager {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        let _ = self.actions.send(Action::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

struct Actor {
    downloads: SharedDownloads,
    root: PathBuf,
    paths: StoragePaths,
    instances: Vec<Instance>,
    status: HashMap<Uuid, Status>,
    children: HashMap<Uuid, OwnedChild>,
    probing: HashMap<Uuid, u32>,
    remote_checks: HashMap<Uuid, Instant>,
    remote_probing: HashMap<Uuid, String>,
    remote_failures: HashMap<Uuid, u8>,
    remote_languages: HashMap<Uuid, String>,
    remote_workers: Vec<JoinHandle<()>>,
    installing: Option<Uuid>,
    installer: Option<JoinHandle<()>>,
    send: Sender<Event>,
    last_snapshot: Option<Vec<View>>,
    logs: SyncSender<Log>,
    internal_send: Sender<Internal>,
    internal: Receiver<Internal>,
    cancel: Arc<AtomicBool>,
    _lock: File,
}
impl Actor {
    fn new(
        paths: StoragePaths,
        instances: Vec<Instance>,
        send: Sender<Event>,
        logs: SyncSender<Log>,
        cancel: Arc<AtomicBool>,
        lock: File,
        downloads: SharedDownloads,
    ) -> Self {
        let root = paths.root.clone();
        let (internal_send, internal) = mpsc::channel();
        let status = instances
            .iter()
            .map(|i| {
                (
                    i.id,
                    if i.is_remote() {
                        Status::RemoteChecking
                    } else if runtime::ready(&runtime::instance_dir(&root, i.id), i.device) {
                        Status::Stopped
                    } else {
                        Status::NotInstalled
                    },
                )
            })
            .collect();
        Self {
            downloads,
            root,
            paths,
            instances,
            status,
            children: HashMap::new(),
            probing: HashMap::new(),
            remote_checks: HashMap::new(),
            remote_probing: HashMap::new(),
            remote_failures: HashMap::new(),
            remote_languages: HashMap::new(),
            remote_workers: Vec::new(),
            installing: None,
            installer: None,
            send,
            last_snapshot: None,
            logs,
            internal_send,
            internal,
            cancel,
            _lock: lock,
        }
    }
    fn logger(&self, id: Uuid) -> Logger {
        let send = self.logs.clone();
        let downloads = self.downloads.clone();
        Arc::new(move |text| {
            if let Some(progress) = ModelDownload::parse(&text) {
                let mut downloads = downloads.lock().unwrap();
                let models = downloads.entry(id).or_default();
                if models.len() >= 256 && !models.contains_key(&progress.key()) {
                    let old = models
                        .iter()
                        .filter(|(_, item)| !item.phase.active())
                        .min_by_key(|(_, item)| item.kind != downloads::DownloadKind::Runtime)
                        .map(|(key, _)| key.clone());
                    if let Some(old) = old {
                        models.remove(&old);
                    } else {
                        return;
                    }
                }
                models.insert(progress.key(), progress);
                return;
            }
            let _ = send.try_send(Log { id, text });
        })
    }
    fn snapshot(&mut self, force: bool) {
        let views = self
            .instances
            .iter()
            .map(|i| View {
                instance: i.clone(),
                status: self.status[&i.id],
                pid: self.children.get(&i.id).map(|c| c.0.id()),
            })
            .collect();
        if force || self.last_snapshot.as_ref() != Some(&views) {
            let _ = self.send.send(Event::Snapshot(views.clone()));
            self.last_snapshot = Some(views);
        }
    }
    fn run(mut self, actions: Receiver<Action>, cleanup: Sender<crate::removals::Removal>) {
        self.snapshot(false);
        let mut last_poll = Instant::now();
        loop {
            match actions.recv_timeout(Duration::from_millis(100)) {
                Ok(Action::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Ok(action) => {
                    let save = if let Action::Save(instance) = &action {
                        Some(instance.id)
                    } else {
                        None
                    };
                    if let Err(error) = self.action(action, &cleanup) {
                        let details = format!("{error:#}");
                        let event = save.map_or_else(
                            || Event::Error(details.clone()),
                            |id| Event::SaveFailed {
                                id,
                                details: details.clone(),
                            },
                        );
                        let _ = self.send.send(event);
                    }
                    self.snapshot(true);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            while let Ok(event) = self.internal.try_recv() {
                let models_updated = matches!(&event, Internal::ModelsUpdated(_, _));
                let storage_cleaned = matches!(&event, Internal::StorageCleaned(_, _));
                match event {
                    Internal::RemoteHealth(id, url, healthy) => {
                        self.remote_health(id, &url, healthy);
                    }
                    Internal::Languages(id, url, languages) => {
                        if self
                            .get(id)
                            .is_ok_and(|instance| instance.is_remote() && instance.url() == url)
                        {
                            self.remote_languages.insert(id, url.clone());
                            let _ = self.send.send(Event::Languages(id, url, languages));
                        }
                    }
                    Internal::Installed(id, result)
                    | Internal::ModelsUpdated(id, result)
                    | Internal::StorageCleaned(id, result) => {
                        if let Some(worker) = self.installer.take() {
                            let _ = worker.join();
                        }
                        self.installing = None;
                        downloads::interrupt(&self.downloads, id);
                        self.status.insert(
                            id,
                            if result.is_ok() {
                                Status::Stopped
                            } else {
                                Status::Failed
                            },
                        );
                        if result.is_ok() && models_updated {
                            let _ = fs::remove_file(self.paths.cache.join("gui-model-index.json"));
                            let _ = self.send.send(Event::Notice(Notice::ModelsUpdated));
                        }
                        if result.is_ok() && storage_cleaned {
                            let _ = self.send.send(Event::Notice(Notice::StorageCleaned));
                        }
                        if let Err(error) = result {
                            (self.logger(id))(error.clone());
                            let _ = self.send.send(Event::Error(error));
                        }
                    }
                    Internal::Health(id, pid, ready) => {
                        if self.probing.get(&id) == Some(&pid) {
                            self.probing.remove(&id);
                        }
                        if self.children.get(&id).is_some_and(|c| c.0.id() == pid) {
                            let old = self.status[&id];
                            self.status.insert(
                                id,
                                if ready {
                                    Status::Running
                                } else if old == Status::Starting {
                                    Status::Starting
                                } else {
                                    Status::Unresponsive
                                },
                            );
                        }
                    }
                }
                self.snapshot(false);
            }
            if last_poll.elapsed() >= Duration::from_secs(1) {
                self.poll();
                self.snapshot(false);
                last_poll = Instant::now();
            }
        }
        self.cancel.store(true, Ordering::Relaxed);
        self.children.clear(); // OwnedChild always kills and reaps.
        if let Some(worker) = self.installer.take() {
            let _ = worker.join();
        }
        for worker in self.remote_workers {
            let _ = worker.join();
        }
    }
    fn get(&self, id: Uuid) -> Result<Instance> {
        self.instances
            .iter()
            .find(|i| i.id == id)
            .cloned()
            .context("This instance no longer exists.")
    }
    fn flush(&mut self, drafts: Vec<Instance>) -> Result<()> {
        if drafts.is_empty() {
            return Ok(());
        }
        let mut next = self.instances.clone();
        for draft in drafts {
            let existing = next
                .iter_mut()
                .find(|instance| instance.id == draft.id)
                .context("The edited instance no longer exists")?;
            if *existing != draft {
                ensure!(
                    !self.children.contains_key(&draft.id) && self.installing != Some(draft.id),
                    "Stop the instance before saving its settings."
                );
                *existing = draft;
            }
        }
        let all = next.clone();
        for draft in &mut next {
            draft.validate_for_save(&all)?;
        }
        model::save(&self.root, &next)?;
        self.instances = next;
        Ok(())
    }

    fn action(&mut self, action: Action, cleanup: &Sender<crate::removals::Removal>) -> Result<()> {
        match action {
            Action::Flush(drafts, reply) => {
                let result = self.flush(drafts).map_err(|error| format!("{error:#}"));
                let _ = reply.send(result);
            }
            Action::Save(instance) => {
                let mut instance = *instance;
                ensure!(
                    !self.children.contains_key(&instance.id)
                        && self.installing != Some(instance.id),
                    "Stop the instance before editing its settings."
                );
                instance.validate_for_save(&self.instances)?;
                let unchanged_endpoint = self.instances.iter().any(|old| {
                    old.id == instance.id && old.is_remote() && old.url() == instance.url()
                });
                let mut next = self.instances.clone();
                if let Some(existing) = next.iter_mut().find(|i| i.id == instance.id) {
                    *existing = instance.clone();
                } else {
                    ensure!(next.len() < 32, "A maximum of 32 instances is supported.");
                    next.push(instance.clone());
                }
                model::save(&self.root, &next)?;
                self.status.insert(
                    instance.id,
                    if instance.is_remote() {
                        if unchanged_endpoint {
                            self.status
                                .get(&instance.id)
                                .copied()
                                .unwrap_or(Status::RemoteChecking)
                        } else {
                            Status::RemoteChecking
                        }
                    } else if runtime::ready(
                        &runtime::instance_dir(&self.root, instance.id),
                        instance.device,
                    ) {
                        Status::Stopped
                    } else {
                        Status::NotInstalled
                    },
                );
                if !unchanged_endpoint {
                    self.remote_checks.remove(&instance.id);
                    self.remote_probing.remove(&instance.id);
                    self.remote_failures.remove(&instance.id);
                    self.remote_languages.remove(&instance.id);
                }
                self.instances = next;
                let _ = self.send.send(Event::Notice(Notice::Saved(instance.id)));
            }
            Action::Install(id) => {
                let instance = self.get(id)?;
                ensure!(
                    !instance.is_remote(),
                    "Hosted instances do not have a local runtime."
                );
                ensure!(
                    self.installing.is_none(),
                    "Wait for the current installation to finish."
                );
                ensure!(
                    !self.children.contains_key(&id),
                    "Stop the instance before installing."
                );
                self.downloads.lock().unwrap().remove(&id);
                self.installing = Some(id);
                self.status.insert(id, Status::Installing);
                let paths = self.paths.clone();
                let send = self.internal_send.clone();
                let cancel = self.cancel.clone();
                let log = self.logger(id);
                self.installer = Some(thread::spawn(move || {
                    let result = runtime::install(&paths, &instance, &cancel, &log)
                        .map_err(|e| format!("Installation failed: {e:#}"));
                    let _ = send.send(Internal::Installed(id, result));
                }));
            }
            Action::UpdateModels(id) | Action::CleanStorage(id) => {
                let clean = matches!(action, Action::CleanStorage(_));
                let instance = self.get(id)?;
                ensure!(
                    !instance.is_remote(),
                    "Hosted models are managed by the service."
                );
                ensure!(
                    self.installing.is_none(),
                    "Wait for the current installation or model update."
                );
                ensure!(
                    !self.children.contains_key(&id),
                    "Stop the instance before updating models."
                );
                ensure!(
                    runtime::installed(&runtime::instance_dir(&self.root, id)),
                    "Install the runtime before updating models."
                );
                self.downloads.lock().unwrap().remove(&id);
                self.installing = Some(id);
                self.status.insert(id, Status::UpdatingModels);
                let paths = self.paths.clone();
                let cancel = self.cancel.clone();
                let send = self.internal_send.clone();
                let log = self.logger(id);
                self.installer = Some(thread::spawn(move || {
                    let result = if clean {
                        runtime::clean_storage(&paths, &instance, &cancel, &log)
                    } else {
                        runtime::update_models(&paths, &instance, &cancel, &log)
                    }
                    .map_err(|error| format!("Model update failed: {error:#}"));
                    let _ = send.send(if clean {
                        Internal::StorageCleaned(id, result)
                    } else {
                        Internal::ModelsUpdated(id, result)
                    });
                }));
            }
            Action::Start(id) => {
                let instance = self.get(id)?;
                ensure!(
                    !instance.is_remote(),
                    "Hosted instances are managed by the service."
                );
                ensure!(
                    self.installing != Some(id),
                    "Wait for installation to finish."
                );
                ensure!(
                    !self.children.contains_key(&id),
                    "This instance is already running."
                );
                self.downloads.lock().unwrap().remove(&id);
                match runtime::start(&self.paths, &instance, &self.logger(id)) {
                    Ok(child) => {
                        self.children.insert(id, child);
                        self.status.insert(id, Status::Starting);
                    }
                    Err(e) => {
                        self.status.insert(id, Status::Failed);
                        (self.logger(id))(format!("{e:#}"));
                        return Err(e);
                    }
                }
            }
            Action::Stop(id) => {
                ensure!(
                    !self.get(id)?.is_remote(),
                    "Hosted instances are managed by the service."
                );
                if let Some(mut child) = self.children.remove(&id) {
                    if let Err(error) = child.0.kill()
                        && child.0.try_wait()?.is_none()
                    {
                        self.children.insert(id, child);
                        bail!("Could not stop the process: {error}");
                    }
                    let _ = child.0.wait();
                    drop(child);
                    let port = self.get(id)?.port;
                    if !runtime::wait_for_port_close(port, Duration::from_secs(5)) {
                        self.status.insert(id, Status::Failed);
                        bail!(
                            "The supervised process stopped, but port {port} is still in use. Wait a moment or choose another port."
                        );
                    }
                    (self.logger(id))("Server stopped.".into());
                }
                self.probing.remove(&id);
                downloads::interrupt(&self.downloads, id);
                if self.installing != Some(id) {
                    self.status.insert(
                        id,
                        if runtime::ready(
                            &runtime::instance_dir(&self.root, id),
                            self.get(id)?.device,
                        ) {
                            Status::Stopped
                        } else {
                            Status::NotInstalled
                        },
                    );
                }
            }
            Action::Remove(id) => {
                let instance = self.get(id)?;
                ensure!(
                    !self.children.contains_key(&id) && self.installing != Some(id),
                    "Stop the instance before removing it."
                );
                let next: Vec<_> = self
                    .instances
                    .iter()
                    .filter(|i| i.id != id)
                    .cloned()
                    .collect();
                let removal = if instance.is_remote() {
                    None
                } else {
                    crate::removals::Removal::stage(&self.root, id)?
                };
                if let Err(error) = model::save(&self.root, &next) {
                    if let Some(removal) = removal {
                        removal.rollback()?;
                    }
                    return Err(error
                        .context("Could not save the instance removal; its files were restored"));
                }
                self.instances = next;
                self.status.remove(&id);
                self.remote_checks.remove(&id);
                self.remote_probing.remove(&id);
                self.remote_failures.remove(&id);
                self.remote_languages.remove(&id);
                self.downloads.lock().unwrap().remove(&id);
                if let Some(removal) = removal {
                    cleanup.send(removal).map_err(|_|anyhow::anyhow!("The instance was removed. Its retained files will be cleaned on restart."))?;
                }
                let _ = self.send.send(Event::Notice(Notice::Removed));
            }
            Action::CheckConnection(id) => {
                let instance = self.get(id)?;
                ensure!(
                    instance.is_remote(),
                    "Connection checks are for remote instances."
                );
                if !self.remote_probing.contains_key(&id) {
                    self.check_remote(instance);
                }
            }
            Action::Shutdown => {}
        }
        Ok(())
    }
    fn poll(&mut self) {
        let remote = self
            .instances
            .iter()
            .filter(|instance| {
                instance.is_remote()
                    && !self.remote_probing.contains_key(&instance.id)
                    && self.remote_checks.get(&instance.id).is_none_or(|checked| {
                        checked.elapsed() >= Duration::from_secs(instance.health.interval_secs)
                    })
            })
            .cloned()
            .collect::<Vec<_>>();
        for instance in remote {
            self.check_remote(instance);
        }
        self.remote_workers.retain(|worker| !worker.is_finished());
        let ids: Vec<_> = self.children.keys().copied().collect();
        for id in ids {
            let result = self.children.get_mut(&id).unwrap().0.try_wait();
            match result {
                Ok(Some(exit)) => {
                    downloads::interrupt(&self.downloads, id);
                    self.children.remove(&id);
                    self.probing.remove(&id);
                    self.status.insert(id, Status::Failed);
                    (self.logger(id))(format!(
                        "Server exited with {exit}. Review the log, then try starting again."
                    ));
                }
                Err(error) => (self.logger(id))(format!("Could not inspect server: {error}")),
                Ok(None) => {
                    if self.probing.contains_key(&id) {
                        continue;
                    }
                    let pid = self.children[&id].0.id();
                    self.probing.insert(id, pid);
                    let url = self.instances.iter().find(|i| i.id == id).unwrap().url();
                    let send = self.internal_send.clone();
                    thread::spawn(move || {
                        let ready = healthy(&url);
                        let _ = send.send(Internal::Health(id, pid, ready));
                    });
                }
            }
        }
    }

    fn remote_health(&mut self, id: Uuid, url: &str, healthy: bool) {
        if self.remote_probing.get(&id).map(String::as_str) != Some(url) {
            return;
        }
        self.remote_probing.remove(&id);
        if let Ok(instance) = self.get(id)
            && instance.is_remote()
            && instance.url() == url
        {
            let failures = self.remote_failures.entry(id).or_default();
            if healthy {
                *failures = 0;
                self.status.insert(id, Status::RemoteOnline);
            } else {
                *failures = failures.saturating_add(1);
                if *failures >= instance.health.failure_threshold {
                    self.status.insert(id, Status::RemoteOffline);
                    self.remote_languages.remove(&id);
                }
            }
        }
    }

    fn check_remote(&mut self, instance: Instance) {
        let id = instance.id;
        let url = instance.url();
        self.remote_checks.insert(id, Instant::now());
        self.remote_probing.insert(id, url.clone());
        let discover = self.remote_languages.get(&id) != Some(&url);
        let send = self.internal_send.clone();
        let cancel = self.cancel.clone();
        self.remote_workers.push(thread::spawn(move || {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let healthy = remote_healthy(&url, Duration::from_secs(instance.health.timeout_secs));
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let _ = send.send(Internal::RemoteHealth(id, url.clone(), healthy));
            if healthy
                && discover
                && !cancel.load(Ordering::Relaxed)
                && let Ok(languages) = crate::translator::languages(&instance)
            {
                let codes = languages
                    .into_iter()
                    .map(|language| language.code)
                    .collect::<Vec<_>>()
                    .join(",");
                let _ = send.send(Internal::Languages(id, url, codes));
            }
        }));
    }
}

fn remote_healthy(url: &str, timeout: Duration) -> bool {
    let check = || -> Result<bool> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout)
            .user_agent("LibreTranslate-GUI")
            .build()?;
        let outcome = runtime.block_on(crate::remote_health::check(&client, url, timeout));
        runtime.shutdown_background();
        Ok(matches!(
            outcome,
            crate::remote_health::ProbeOutcome::Online(_)
        ))
    };
    check().unwrap_or(false)
}

fn healthy(url: &str) -> bool {
    let result = (|| -> Result<bool> {
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_millis(800))
            .build()?;
        let response = client
            .get(format!("{url}/languages"))
            .send()?
            .error_for_status()?;
        use std::io::Read;
        let mut bytes = Vec::new();
        response.take(1024 * 1024).read_to_end(&mut bytes)?;
        let value: serde_json::Value = serde_json::from_slice(&bytes)?;
        Ok(value
            .as_array()
            .is_some_and(|items| items.len() >= 2 && items.iter().all(|i| i["code"].is_string())))
    })();
    result.unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_remote_checks_accept_legacy_languages_fallback_without_accepting_websites() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        let languages = r#"[{"code":"en"},{"code":"es"}]"#;
        for (responses, expected) in [
            (vec![(200, r#"{"status":"ok"}"#)], true),
            (vec![(404, "{}"), (200, languages)], true),
            (vec![(405, "{}"), (200, languages)], true),
            (vec![(404, "{}"), (200, "[]")], false),
            (vec![(404, "{}"), (200, "<html>Login</html>")], false),
            (vec![(404, "{}"), (200, r#"[{"code":"bad code"}]"#)], false),
            (vec![(503, "{}")], false),
            (vec![(200, "<html>Website</html>")], false),
            (vec![(302, "{}")], false),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            listener.set_nonblocking(true).unwrap();
            let steps = responses.len();
            let worker = thread::spawn(move || {
                let mut paths = Vec::new();
                for (status, body) in responses {
                    let deadline = Instant::now() + Duration::from_secs(3);
                    let mut stream = loop {
                        match listener.accept() {
                            Ok((stream, _)) => break stream,
                            Err(error)
                                if error.kind() == std::io::ErrorKind::WouldBlock
                                    && Instant::now() < deadline =>
                            {
                                thread::sleep(Duration::from_millis(2))
                            }
                            Err(error) => panic!("Health fixture connection failed: {error}"),
                        }
                    };
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut request = Vec::new();
                    let mut buffer = [0; 2048];
                    while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                        let size = stream.read(&mut buffer).unwrap();
                        assert!(size > 0 && request.len() + size <= 8192);
                        request.extend_from_slice(&buffer[..size]);
                    }
                    paths.push(
                        String::from_utf8(request)
                            .unwrap()
                            .lines()
                            .next()
                            .unwrap()
                            .to_owned(),
                    );
                    let reply = format!(
                        "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\nLocation: http://127.0.0.1:1/other\r\n\r\n{body}",
                        body.len()
                    );
                    stream.write_all(reply.as_bytes()).unwrap();
                }
                paths
            });
            assert_eq!(
                remote_healthy(&format!("http://{address}/prefix/"), Duration::from_secs(1)),
                expected
            );
            let paths = worker.join().unwrap();
            assert_eq!(paths[0], "GET /prefix/health HTTP/1.1");
            if steps == 2 {
                assert_eq!(paths[1], "GET /prefix/languages HTTP/1.1");
            }
        }
    }

    #[test]
    fn manual_health_checks_preserve_translation_until_failure_threshold() {
        let root = tempfile::tempdir().unwrap();
        let instance = Instance {
            remote_url: "http://127.0.0.1:9/prefix".into(),
            ..Instance::remote()
        };
        let id = instance.id;
        let url = instance.url();
        let (send, _events) = mpsc::channel();
        let (logs, _log_events) = mpsc::sync_channel(1);
        // Exercise actor transitions without letting the probe threads use a network.
        let mut actor = Actor::new(
            StoragePaths::new(root.path().into()),
            vec![instance],
            send,
            logs,
            Arc::new(AtomicBool::new(true)),
            File::create(root.path().join("manager.lock")).unwrap(),
            SharedDownloads::default(),
        );
        let (cleanup, _removals) = mpsc::channel();
        actor.action(Action::CheckConnection(id), &cleanup).unwrap();
        assert_eq!(actor.status[&id], Status::RemoteChecking);
        actor.remote_health(id, &url, true);
        assert!(actor.status[&id].can_translate());

        actor.action(Action::CheckConnection(id), &cleanup).unwrap();
        assert!(
            actor.status[&id].can_translate(),
            "Manual probes must not interrupt transfers"
        );
        actor.remote_health(id, &url, false);
        assert!(
            actor.status[&id].can_translate(),
            "One failed check is below the threshold"
        );
        actor.action(Action::CheckConnection(id), &cleanup).unwrap();
        assert!(actor.status[&id].can_translate());
        actor.remote_health(id, &url, false);
        assert_eq!(actor.status[&id], Status::RemoteOffline);

        actor.action(Action::CheckConnection(id), &cleanup).unwrap();
        assert_eq!(actor.status[&id], Status::RemoteOffline);
        actor.remote_health(id, "http://other.example", true);
        assert_eq!(actor.status[&id], Status::RemoteOffline);
        actor.remote_health(id, &url, true);
        assert!(actor.status[&id].can_translate());
        assert_eq!(actor.remote_failures[&id], 0);
        for worker in actor.remote_workers {
            worker.join().unwrap();
        }
    }

    #[test]
    fn hosted_instance_is_editable_removable_and_never_spawns_a_runtime() {
        let dir = tempfile::tempdir().unwrap();
        model::save(dir.path(), &[Instance::hosted()]).unwrap();
        let manager = Manager::new(dir.path().into()).unwrap();
        let view = wait(&manager, |views| views.len() == 1).remove(0);
        assert_eq!(view.status, Status::RemoteChecking);
        assert!(!view.status.can_translate());
        assert!(!view.status.busy());
        assert!(view.pid.is_none());
        let mut instance = view.instance;
        let id = instance.id;
        instance.server.api_key = "test-key".into();
        manager
            .actions
            .send(Action::Save(Box::new(instance)))
            .unwrap();
        wait(&manager, |views| {
            views[0].instance.server.api_key == "test-key"
        });
        for action in [
            Action::Start(id),
            Action::Stop(id),
            Action::Install(id),
            Action::UpdateModels(id),
            Action::CleanStorage(id),
        ] {
            manager.actions.send(action).unwrap();
            loop {
                if matches!(
                    manager.events.recv_timeout(Duration::from_secs(5)).unwrap(),
                    Event::Error(_)
                ) {
                    break;
                }
            }
        }
        assert!(!runtime::instance_dir(dir.path(), id).exists());
        manager.actions.send(Action::Remove(id)).unwrap();
        wait(&manager, |views| views.is_empty());
        drop(manager);
        let manager = Manager::new(dir.path().into()).unwrap();
        wait(&manager, |views| views.is_empty());
        assert!(
            !fs::read_to_string(dir.path().join("instances.json"))
                .unwrap()
                .contains("test-key")
        );
        // Re-adding uses the same ordinary Save operation as a local instance.
        let mut hosted = Instance::hosted();
        hosted.server.api_key = "new-test-key".into();
        manager
            .actions
            .send(Action::Save(Box::new(hosted)))
            .unwrap();
        wait(&manager, |views| {
            views.len() == 1 && views[0].status == Status::RemoteChecking
        });
    }

    #[test]
    fn busy_renderer_does_not_lose_save_acknowledgements() {
        let dir = tempfile::tempdir().unwrap();
        model::save(dir.path(), &[]).unwrap();
        let manager = Manager::new(dir.path().into()).unwrap();
        let mut instance = Instance::default();
        for index in 0..40 {
            instance.name = format!("Revision {index}");
            manager
                .actions
                .send(Action::Save(Box::new(instance.clone())))
                .unwrap();
        }
        let started = Instant::now();
        while model::load(dir.path())
            .ok()
            .and_then(|instances| instances.first().map(|instance| instance.name.clone()))
            .as_deref()
            != Some("Revision 39")
        {
            assert!(started.elapsed() < Duration::from_secs(10));
            thread::sleep(Duration::from_millis(20));
        }
        // The renderer deliberately did not drain its event queue during these saves.
        let mut saved = 0;
        let mut final_snapshot = false;
        while saved < 40 || !final_snapshot {
            match manager.events.recv_timeout(Duration::from_secs(2)).unwrap() {
                Event::Notice(Notice::Saved(_)) => saved += 1,
                Event::Snapshot(views) => {
                    final_snapshot = views
                        .first()
                        .is_some_and(|view| view.instance.name == "Revision 39")
                }
                _ => {}
            }
        }
        assert_eq!(saved, 40);
        while manager.events.try_recv().is_ok() {}
        assert!(
            manager
                .events
                .recv_timeout(Duration::from_millis(1200))
                .is_err()
        );
    }

    /// Opt-in test: downloads Python, packages, and English/Spanish models.
    #[test]
    #[ignore = "downloads the managed runtime and language models; requires internet"]
    fn real_runtime_translation() {
        let base = std::env::current_dir().unwrap().join(".runtime-test");
        fs::create_dir_all(&base).unwrap();
        let root = tempfile::tempdir_in(&base).unwrap();
        let log: Logger = Arc::new(|line| println!("{line}"));
        let instance = Instance {
            device: if std::env::var_os("LIBRETRANSLATE_GUI_TEST_CUDA").is_some() {
                crate::model::Device::Cuda
            } else {
                crate::model::Device::Cpu
            },
            port: {
                let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
                listener.local_addr().unwrap().port()
            },
            ..Default::default()
        };
        let paths = StoragePaths {
            root: root.path().to_path_buf(),
            cache: base.join("package-cache"),
            python: base.join("python"),
        };
        println!("Testing translation device: {:?}", instance.device);
        runtime::install(&paths, &instance, &AtomicBool::new(false), &log).unwrap();
        let dir = runtime::instance_dir(&paths.root, instance.id);
        assert!(dir.join("model-languages.json").is_file());
        let inventory = crate::models::inspect(
            &paths.root,
            &paths.cache,
            instance.id,
            &instance.languages,
            false,
        )
        .unwrap();
        assert_eq!(inventory.rows.len(), 2);
        assert!(inventory.rows.iter().all(|row| row.installed.is_some()));
        assert!(std::net::TcpStream::connect(("127.0.0.1", instance.port)).is_err());
        let mut process = runtime::start(&paths, &instance, &log).unwrap();
        let deadline = Instant::now() + Duration::from_secs(600);
        while !healthy(&instance.url()) {
            assert!(
                process.0.try_wait().unwrap().is_none(),
                "LibreTranslate exited during startup"
            );
            assert!(
                Instant::now() < deadline,
                "LibreTranslate never became ready"
            );
            thread::sleep(Duration::from_secs(1));
        }
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap();
        let value: serde_json::Value = client
            .post(format!("{}/translate", instance.url()))
            .json(&serde_json::json!({"q":"Hello", "source":"en", "target":"es", "format":"text"}))
            .send()
            .unwrap()
            .error_for_status()
            .unwrap()
            .json()
            .unwrap();
        println!("Translation response: {value}");
        assert!(
            value["translatedText"]
                .as_str()
                .is_some_and(|text| text.to_lowercase().contains("hola"))
        );
        drop(client);
        drop(process);
        assert!(
            runtime::wait_for_port_close(instance.port, Duration::from_secs(5)),
            "Runtime is still accepting connections after shutdown"
        );
        let mut restarted = runtime::start(&paths, &instance, &log).unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        while !healthy(&instance.url()) {
            assert!(
                restarted.0.try_wait().unwrap().is_none(),
                "LibreTranslate exited during restart"
            );
            assert!(Instant::now() < deadline, "Restart never became ready");
            thread::sleep(Duration::from_millis(200));
        }
        drop(restarted);
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "uses an existing local runtime and models without installing or removing files"]
    fn existing_romanian_runtime_handles_canonical_windows_paths() {
        let root = PathBuf::from(
            std::env::var_os("LIBRETRANSLATE_GUI_EXISTING_TEST_DATA_ROOT")
                .expect("Set the existing runtime data root"),
        );
        let mut instance = model::load(&root)
            .unwrap()
            .into_iter()
            .find(|instance| {
                instance.languages.split(',').any(|code| code == "ro")
                    && instance.languages.split(',').any(|code| code == "en")
            })
            .expect("Needs installed en/ro models");
        instance.port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap().port()
        };
        let paths = StoragePaths::new(root.canonicalize().unwrap());
        let log: Logger = Arc::new(|line| println!("{line}"));
        let mut child = runtime::start(&paths, &instance, &log).unwrap();
        let deadline = Instant::now() + Duration::from_secs(120);
        while !healthy(&instance.url()) {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "Server exited before readiness"
            );
            assert!(Instant::now() < deadline, "Server did not become ready");
            thread::sleep(Duration::from_millis(200));
        }
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(60))
            .build()
            .unwrap();
        for (text, source, target) in [("Hello", "en", "ro"), ("Salut", "ro", "en")] {
            let value: serde_json::Value = client
                .post(format!("{}/translate", instance.url()))
                .json(
                    &serde_json::json!({"q":text,"source":source,"target":target,"format":"text"}),
                )
                .send()
                .unwrap()
                .error_for_status()
                .unwrap()
                .json()
                .unwrap();
            println!("{source}->{target}: {value}");
            assert!(
                value["translatedText"]
                    .as_str()
                    .is_some_and(|translated| !translated.is_empty() && translated != text)
            );
        }
        drop(child);
        assert!(runtime::wait_for_port_close(
            instance.port,
            Duration::from_secs(5)
        ));
    }

    fn wait(manager: &Manager, predicate: impl Fn(&[View]) -> bool) -> Vec<View> {
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end {
            if let Ok(Event::Snapshot(views)) =
                manager.events.recv_timeout(Duration::from_millis(100))
                && predicate(&views)
            {
                return views;
            }
        }
        panic!("Timed out waiting for manager state");
    }
    #[test]
    fn audit_failed_removal_save_preserves_instance_files() {
        let dir = tempfile::tempdir().unwrap();
        model::save(dir.path(), &[]).unwrap();
        let manager = Manager::new(dir.path().into()).unwrap();
        let instance = Instance::default();
        let id = instance.id;
        manager
            .actions
            .send(Action::Save(Box::new(instance)))
            .unwrap();
        wait(&manager, |views| views.len() == 1);
        let files = runtime::instance_dir(&manager.root, id);
        fs::create_dir_all(&files).unwrap();
        fs::write(files.join("model.bin"), b"keep this model").unwrap();
        fs::create_dir(manager.root.join("instances.json.tmp")).unwrap();
        manager.actions.send(Action::Remove(id)).unwrap();
        loop {
            if matches!(
                manager.events.recv_timeout(Duration::from_secs(5)).unwrap(),
                Event::Error(_)
            ) {
                break;
            }
        }
        assert_eq!(model::load(dir.path()).unwrap().len(), 1);
        assert_eq!(
            fs::read(files.join("model.bin")).unwrap(),
            b"keep this model"
        );
    }
    #[test]
    fn shutdown_flush_acknowledges_failure_and_can_retry_without_losing_drafts() {
        let dir = tempfile::tempdir().unwrap();
        model::save(dir.path(), &[]).unwrap();
        let manager = Manager::new(dir.path().into()).unwrap();
        let mut instance = Instance::default();
        manager
            .actions
            .send(Action::Save(Box::new(instance.clone())))
            .unwrap();
        wait(&manager, |views| views.len() == 1);
        instance.name = "Unsaved change".into();
        fs::create_dir(manager.root.join("instances.json.tmp")).unwrap();
        let (send, receive) = mpsc::channel();
        manager
            .actions
            .send(Action::Flush(vec![instance.clone()], send))
            .unwrap();
        assert!(
            receive
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .is_err()
        );
        assert_ne!(model::load(dir.path()).unwrap()[0].name, instance.name);
        fs::remove_dir(manager.root.join("instances.json.tmp")).unwrap();
        let (send, receive) = mpsc::channel();
        manager
            .actions
            .send(Action::Flush(vec![instance.clone()], send))
            .unwrap();
        receive
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        assert_eq!(model::load(dir.path()).unwrap()[0].name, instance.name);
    }
    #[test]
    fn persists_instances_locks_directory_and_removes_files() {
        let dir = tempfile::tempdir().unwrap();
        model::save(dir.path(), &[]).unwrap();
        let manager = Manager::new(dir.path().into()).unwrap();
        assert!(Manager::new(dir.path().into()).is_err());
        let instance = Instance::default();
        let id = instance.id;
        manager
            .actions
            .send(Action::Save(Box::new(instance)))
            .unwrap();
        assert_eq!(
            wait(&manager, |v| v.len() == 1)[0].status,
            Status::NotInstalled
        );
        let files = runtime::instance_dir(&manager.root, id);
        fs::create_dir_all(&files).unwrap();
        fs::write(files.join("model"), "test").unwrap();
        manager.actions.send(Action::Remove(id)).unwrap();
        wait(&manager, |v| v.is_empty());
        assert!(!files.exists());
        drop(manager);
        assert!(model::load(dir.path()).unwrap().is_empty());
        drop(Manager::new(dir.path().into()).unwrap());
    }
    #[test]
    fn readiness_requires_valid_languages_response() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        for (body, expected) in [
            (r#"[{"code":"en"},{"code":"es"}]"#, true),
            (r#"{"error":"loading"}"#, false),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            let worker = thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut buf = [0; 2048];
                let mut received = Vec::new();
                while !received.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let count = stream.read(&mut buf).unwrap();
                    assert!(
                        count > 0 && received.len() + count <= 8192,
                        "incomplete HTTP probe request"
                    );
                    received.extend_from_slice(&buf[..count]);
                }
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .unwrap();
            });
            assert_eq!(healthy(&format!("http://{address}")), expected);
            worker.join().unwrap();
        }
    }
}
