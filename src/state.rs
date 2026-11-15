use crate::{
    downloads::ModelDownload,
    gpu::GpuInfo,
    i18n::Locale,
    manager::{Action, Event, Manager, Notice, View},
    model::Instance,
    settings::{Preferences, Settings, StoragePaths, Theme},
};
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use uuid::Uuid;

pub type Backend = Arc<Mutex<Option<Manager>>>;
pub type PendingSaves = Arc<Mutex<HashMap<Uuid, (Instance, Instant)>>>;
pub type InitialState = Arc<Mutex<Option<AppState>>>;
pub fn flush_pending(backend: &Backend, pending: &PendingSaves) -> anyhow::Result<()> {
    let drafts = pending
        .lock()
        .unwrap()
        .values()
        .map(|(draft, _)| draft.clone())
        .collect::<Vec<_>>();
    if drafts.is_empty() {
        return Ok(());
    }
    let (reply, result) = std::sync::mpsc::channel();
    {
        let guard = backend.lock().unwrap();
        let manager = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("The instance manager is unavailable"))?;
        manager.actions.send(Action::Flush(drafts, reply))?;
    }
    result.recv()?.map_err(anyhow::Error::msg)
}
pub struct PollUpdate {
    events: Vec<Event>,
    logs: Vec<crate::manager::Log>,
    downloads: Option<HashMap<Uuid, BTreeMap<String, ModelDownload>>>,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Instances,
    Options,
}
#[derive(Clone)]
pub struct Message {
    pub key: &'static str,
    pub details: Option<String>,
    pub error: bool,
}
pub struct AppState {
    pub closing: bool,
    pub close_requested: Arc<std::sync::atomic::AtomicBool>,
    pub sidebar_expanded: bool,
    pub pending_saves: PendingSaves,
    pub saving: Option<Instance>,
    saving_revision: Option<Instant>,
    saved_at: HashMap<Uuid, Instant>,
    failed_saves: HashMap<Uuid, (Instant, Instant)>,
    pub backend: Backend,
    pub settings: Option<Settings>,
    pub preferences: Preferences,
    pub active_paths: Option<StoragePaths>,
    pub locale: Locale,
    pub page: Page,
    pub views: Vec<View>,
    pub selected: Option<Uuid>,
    pub editor: Option<Instance>,
    pub adding: bool,
    pub logs: HashMap<Uuid, VecDeque<String>>,
    pub downloads: HashMap<Uuid, BTreeMap<String, ModelDownload>>,
    pub translations: HashMap<Uuid, crate::translator::Draft>,
    pub file_translations: HashMap<Uuid, crate::translator_files::Draft>,
    pub message: Option<Message>,
    pub fatal: Option<String>,
    pub pending: bool,
    pub filter: String,
    pub remove: Option<Uuid>,
    pub gpu: Option<GpuInfo>,
    pub releases: Option<crate::releases::ReleaseCatalog>,
    pub release_checking: bool,
    pub release_refresh: u32,
    pub app_update: Option<crate::app_updates::Release>,
    pub app_update_open: bool,
    pub app_update_checking: bool,
    pub app_update_refresh: u32,
    pub app_update_status: Option<&'static str>,
    pub app_update_error: Option<String>,
}
impl AppState {
    pub fn new() -> Self {
        Self::from_settings(Settings::load())
    }
    fn from_settings(loaded: anyhow::Result<Settings>) -> Self {
        let mut fatal = None;
        let settings = match loaded {
            Ok(settings) => Some(settings),
            Err(error) => {
                fatal = Some(format!("{error:#}"));
                None
            }
        };
        let preferences = settings
            .as_ref()
            .map(|s| s.preferences.clone())
            .unwrap_or_default();
        let active_paths = settings.as_ref().and_then(|s| match s.paths(&preferences) {
            Ok(paths) => Some(paths),
            Err(error) => {
                fatal = Some(format!("{error:#}"));
                None
            }
        });
        let manager =
            active_paths
                .as_ref()
                .and_then(|paths| match Manager::with_paths(paths.clone()) {
                    Ok(manager) => Some(manager),
                    Err(error) => {
                        fatal = Some(format!("{error:#}"));
                        None
                    }
                });
        let update_error = crate::self_update::previous_failure();
        Self {
            closing: false,
            close_requested: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            sidebar_expanded: false,
            pending_saves: Arc::new(Mutex::new(HashMap::new())),
            saving: None,
            saving_revision: None,
            saved_at: HashMap::new(),
            failed_saves: HashMap::new(),
            backend: Arc::new(Mutex::new(manager)),
            settings,
            active_paths,
            locale: preferences.locale,
            preferences,
            page: Page::Instances,
            views: vec![],
            selected: None,
            editor: None,
            adding: false,
            logs: HashMap::new(),
            downloads: HashMap::new(),
            translations: HashMap::new(),
            file_translations: HashMap::new(),
            message: update_error.clone().map(|details| Message {
                key: "app_update_failed",
                details: Some(details),
                error: true,
            }),
            fatal,
            pending: false,
            filter: String::new(),
            remove: None,
            gpu: None,
            releases: None,
            release_checking: false,
            release_refresh: 0,
            app_update: None,
            app_update_open: false,
            app_update_checking: false,
            app_update_refresh: 0,
            app_update_status: None,
            app_update_error: update_error,
        }
    }
    pub fn notice(&mut self, key: &'static str) {
        self.message = Some(Message {
            key,
            details: None,
            error: false,
        });
    }
    pub fn dismiss_downloads(&mut self, item: Option<(Uuid, String)>) {
        let shared = self
            .backend
            .lock()
            .ok()
            .and_then(|backend| backend.as_ref().map(|manager| manager.downloads.clone()));
        if let Some(shared) = shared {
            crate::downloads::dismiss_finished(
                &shared,
                item.as_ref().map(|(id, key)| (*id, key.as_str())),
            );
            self.downloads = shared.lock().unwrap().clone();
        }
    }
    pub fn error(&mut self, key: &'static str, details: impl Into<String>) {
        self.message = Some(Message {
            key,
            details: Some(details.into()),
            error: true,
        });
    }
    pub fn send(&mut self, action: Action) {
        let result = self
            .backend
            .lock()
            .ok()
            .and_then(|backend| backend.as_ref().map(|m| m.actions.send(action)));
        match result {
            Some(Ok(())) => {
                self.pending = true;
                self.message = None;
            }
            _ => self.error("manager_gone", "The background manager is unavailable."),
        }
    }
    pub fn collect_poll(&self) -> Option<PollUpdate> {
        let backend = self.backend.lock().ok()?;
        let manager = backend.as_ref()?;
        let events = manager.events.try_iter().filter(|event| !matches!(event, Event::Snapshot(views) if !self.pending && *views==self.views)).collect::<Vec<_>>();
        let logs = manager.logs.try_iter().take(1024).collect::<Vec<_>>();
        let downloads = {
            let downloads = manager.downloads.lock().unwrap();
            (*downloads != self.downloads).then(|| downloads.clone())
        };
        let expires = self
            .saved_at
            .values()
            .any(|time| time.elapsed() >= Duration::from_secs(2));
        (!events.is_empty()
            || !logs.is_empty()
            || downloads.is_some()
            || expires
            || self.autosave_due())
        .then_some(PollUpdate {
            events,
            logs,
            downloads,
        })
    }
    pub fn poll(&mut self, update: PollUpdate) {
        self.saved_at
            .retain(|_, time| time.elapsed() < Duration::from_secs(2));
        if let Some(downloads) = update.downloads {
            self.downloads = downloads;
        }
        for event in update.events {
            match event {
                Event::Snapshot(views) => {
                    for view in &views {
                        if view.status == crate::manager::Status::Running
                            && self
                                .views
                                .iter()
                                .find(|old| old.instance.id == view.instance.id)
                                .is_some_and(|old| old.status != view.status || old.pid != view.pid)
                            && let Some(draft) = self.translations.get_mut(&view.instance.id)
                        {
                            draft.server_restarted();
                        }
                    }
                    self.views = views;
                    self.translations
                        .retain(|id, _| self.views.iter().any(|view| view.instance.id == *id));
                    self.file_translations.retain(|id, draft| {
                        let present = self.views.iter().any(|view| view.instance.id == *id);
                        if !present {
                            draft.cancel();
                        }
                        present
                    });
                    self.logs
                        .retain(|id, _| self.views.iter().any(|view| view.instance.id == *id));
                    self.failed_saves
                        .retain(|id, _| self.views.iter().any(|view| view.instance.id == *id));
                    self.saved_at
                        .retain(|id, _| self.views.iter().any(|view| view.instance.id == *id));
                    self.pending = false;
                    if !self.adding
                        && self.selected.is_some()
                        && !self
                            .views
                            .iter()
                            .any(|v| Some(v.instance.id) == self.selected)
                    {
                        self.selected = None;
                        self.editor = None;
                    }
                }
                Event::Error(text) => {
                    self.error("operation_failed", text);
                    self.pending = false;
                }
                Event::SaveFailed { id, details } => {
                    if self.saving.as_ref().is_some_and(|draft| draft.id == id) {
                        let revision = self.saving_revision.take().unwrap_or_else(Instant::now);
                        let already_reported = self
                            .failed_saves
                            .get(&id)
                            .is_some_and(|(failed, _)| *failed == revision);
                        self.failed_saves.insert(id, (revision, Instant::now()));
                        self.saving = None;
                        if !already_reported {
                            self.error("operation_failed", details);
                        }
                    } else {
                        self.error("operation_failed", details);
                    }
                    self.pending = false;
                }
                Event::Notice(Notice::ModelsUpdated) => {
                    self.notice("models_updated");
                    self.pending = false;
                }
                Event::Notice(Notice::Saved(id)) => {
                    self.failed_saves.remove(&id);
                    if self.saving.as_ref().is_some_and(|draft| draft.id == id) {
                        let submitted = self.saving.take().unwrap();
                        let mut pending = self.pending_saves.lock().unwrap();
                        let same_revision = pending
                            .get(&submitted.id)
                            .is_some_and(|(_, changed)| Some(*changed) == self.saving_revision);
                        if same_revision {
                            pending.remove(&submitted.id);
                            if self
                                .editor
                                .as_ref()
                                .is_some_and(|draft| draft.id == submitted.id)
                            {
                                self.editor = None;
                            }
                        }
                        self.saving_revision = None;
                        self.saved_at.insert(submitted.id, Instant::now());
                    } else if self.adding
                        && self.editor.as_ref().is_some_and(|draft| draft.id == id)
                    {
                        self.selected = self.editor.as_ref().map(|i| i.id);
                        self.adding = false;
                        self.editor = None;
                        self.notice("saved");
                    }
                    self.pending = false;
                }
                Event::Notice(Notice::StorageCleaned) => {
                    self.notice("storage_cleaned");
                    self.pending = false;
                }
                Event::Notice(Notice::Removed) => {
                    self.notice("removed");
                    self.pending = false;
                }
            }
        }
        for log in update.logs {
            if !self.views.iter().any(|view| view.instance.id == log.id) {
                continue;
            }
            let lines = self.logs.entry(log.id).or_default();
            lines.push_back(log.text);
            while lines.len() > 600 {
                lines.pop_front();
            }
        }
        for view in &self.views {
            if !matches!(
                view.status,
                crate::manager::Status::Installing
                    | crate::manager::Status::Starting
                    | crate::manager::Status::UpdatingModels
            ) && let Some(models) = self.downloads.get_mut(&view.instance.id)
            {
                for model in models.values_mut().filter(|model| model.phase.active()) {
                    model.phase = crate::downloads::DownloadPhase::Interrupted;
                }
            }
        }
        self.autosave();
    }
    pub fn add(&mut self) {
        let assigned = self
            .views
            .iter()
            .map(|v| v.instance.port)
            .collect::<Vec<_>>();
        let port = crate::network::find_free_port(5000, &assigned).unwrap_or(5000);
        self.editor = Some(Instance {
            name: self.locale.text("new_name").into(),
            port,
            languages: if self.locale == Locale::En {
                "en,es".into()
            } else {
                format!("en,{}", self.locale.translation_code())
            },
            ..Default::default()
        });
        self.adding = true;
        self.page = Page::Instances;
        self.message = None;
    }
    pub fn select(&mut self, id: Uuid) {
        self.selected = Some(id);
        self.editor = self
            .pending_saves
            .lock()
            .unwrap()
            .get(&id)
            .map(|(draft, _)| draft.clone());
        self.adding = false;
        self.page = Page::Instances;
        self.message = None;
        self.remove = None;
    }
    pub fn edit(&mut self, id: Uuid, update: impl FnOnce(&mut Instance)) {
        self.failed_saves.remove(&id);
        if self
            .editor
            .as_ref()
            .is_none_or(|instance| instance.id != id)
        {
            self.editor = self
                .views
                .iter()
                .find(|v| v.instance.id == id)
                .map(|v| v.instance.clone());
        }
        if let Some(instance) = self.editor.as_mut() {
            update(instance);
            if !self.adding {
                self.pending_saves
                    .lock()
                    .unwrap()
                    .insert(id, (instance.clone(), Instant::now()));
            }
        }
    }
    pub fn save_instance(&mut self, mut instance: Instance) {
        let instances: Vec<_> = self.views.iter().map(|v| v.instance.clone()).collect();
        if let Err(error) = instance.validate(&instances) {
            let detail = error.to_string();
            let key = if detail.contains("name") {
                "validation_name"
            } else if detail.contains("already assigned") {
                "validation_port_used"
            } else if detail.contains("port") {
                "validation_port"
            } else if detail.contains("threads") {
                "validation_threads"
            } else {
                "validation_languages"
            };
            self.message = Some(Message {
                key,
                details: None,
                error: true,
            });
            return;
        }
        if instance.device == crate::model::Device::Cuda
            && !self.gpu.as_ref().is_some_and(GpuInfo::available)
        {
            self.message = Some(Message {
                key: "gpu_saved_missing",
                details: None,
                error: true,
            });
            return;
        }
        self.send(Action::Save(Box::new(instance)));
    }
    fn autosave_due(&self) -> bool {
        if self.pending || self.saving.is_some() {
            return false;
        }
        let pending = self.pending_saves.lock().unwrap();
        if pending.is_empty() {
            return false;
        }
        let instances = self
            .views
            .iter()
            .map(|view| view.instance.clone())
            .collect::<Vec<_>>();
        pending.values().any(|(draft, changed)| {
            changed.elapsed() >= Duration::from_millis(600)
                && self.save_retry_ready(draft.id, *changed)
                && self
                    .views
                    .iter()
                    .find(|view| view.instance.id == draft.id)
                    .is_some_and(|view| !view.status.busy() && draft != &view.instance)
                && draft.clone().validate(&instances).is_ok()
                && (draft.device != crate::model::Device::Cuda
                    || self.gpu.as_ref().is_some_and(GpuInfo::available))
        })
    }
    fn autosave(&mut self) {
        if self.closing || self.pending || self.saving.is_some() {
            return;
        }
        let pending = self
            .pending_saves
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let instances = self
            .views
            .iter()
            .map(|view| view.instance.clone())
            .collect::<Vec<_>>();
        for (mut draft, changed) in pending {
            if changed.elapsed() < Duration::from_millis(600)
                || !self.save_retry_ready(draft.id, changed)
            {
                continue;
            }
            let Some(view) = self.views.iter().find(|view| view.instance.id == draft.id) else {
                self.pending_saves.lock().unwrap().remove(&draft.id);
                continue;
            };
            if view.status.busy() {
                continue;
            }
            if draft == view.instance {
                self.pending_saves.lock().unwrap().remove(&draft.id);
                continue;
            }
            if draft.validate(&instances).is_err() {
                continue;
            }
            if draft.device == crate::model::Device::Cuda
                && !self.gpu.as_ref().is_some_and(GpuInfo::available)
            {
                continue;
            }
            self.saving_revision = Some(changed);
            self.saving = Some(draft.clone());
            self.send(Action::Save(Box::new(draft)));
            break;
        }
    }
    pub fn has_unsaved(&self, id: Uuid) -> bool {
        self.pending_saves.lock().unwrap().contains_key(&id)
    }
    fn save_retry_ready(&self, id: Uuid, revision: Instant) -> bool {
        self.failed_saves.get(&id).is_none_or(|(failed, at)| {
            *failed != revision || at.elapsed() >= Duration::from_secs(5)
        })
    }
    pub fn autosave_status(&self, id: Uuid) -> Option<&'static str> {
        if self.saving.as_ref().is_some_and(|draft| draft.id == id) {
            Some("autosave_saving")
        } else if self.has_unsaved(id) {
            Some("autosave_pending")
        } else if self
            .saved_at
            .get(&id)
            .is_some_and(|saved| saved.elapsed() < Duration::from_secs(2))
        {
            Some("autosave_saved")
        } else {
            None
        }
    }
    pub fn form_error(&self, draft: &Instance) -> Option<&'static str> {
        let others = self
            .views
            .iter()
            .map(|v| v.instance.clone())
            .collect::<Vec<_>>();
        let error = draft.clone().validate(&others).err()?.to_string();
        Some(if error.starts_with("Advanced settings:") {
            if error.contains("limits") {
                "validation_limits"
            } else {
                "validation_advanced"
            }
        } else if error.contains("name") {
            "validation_name"
        } else if error.contains("already assigned") {
            "validation_port_used"
        } else if error.contains("port") {
            "validation_port"
        } else if error.contains("threads") {
            "validation_threads"
        } else {
            "validation_languages"
        })
    }
    pub fn set_locale(&mut self, locale: Locale) {
        self.locale = locale;
        self.preferences.locale = locale;
        // Preserve unsaved path edits; changing locale saves only that preference.
        if let Some(settings) = self.settings.as_mut() {
            let mut saved = settings.preferences.clone();
            saved.locale = locale;
            if let Err(error) = settings.save(&saved) {
                self.error("operation_failed", format!("{error:#}"));
            }
        }
    }
    pub fn set_theme(&mut self, theme: Theme) {
        self.preferences.theme = theme;
        if let Some(settings) = self.settings.as_mut() {
            let mut saved = settings.preferences.clone();
            saved.theme = theme;
            if let Err(error) = settings.save(&saved) {
                self.error("operation_failed", format!("{error:#}"));
            }
        }
    }
    pub fn set_follow_logs(&mut self, follow: bool) {
        self.preferences.follow_logs = follow;
        if let Some(settings) = self.settings.as_mut() {
            let mut saved = settings.preferences.clone();
            saved.follow_logs = follow;
            if let Err(error) = settings.save(&saved) {
                self.error("operation_failed", format!("{error:#}"));
            }
        }
    }
    pub fn set_translator_counters(&mut self, counters: crate::settings::TranslatorCounters) {
        self.preferences.translator_counters = counters.clone();
        if let Some(settings) = self.settings.as_mut() {
            let mut saved = settings.preferences.clone();
            saved.translator_counters = counters;
            if let Err(error) = settings.save(&saved) {
                self.error("operation_failed", format!("{error:#}"));
            }
        }
    }
    pub fn save_options(&mut self) {
        let Some(settings) = self.settings.as_mut() else {
            self.error(
                "operation_failed",
                "Restore the preferences file before saving options.",
            );
            return;
        };
        let result = (|| -> anyhow::Result<bool> {
            let paths = settings.paths(&self.preferences)?;
            for directory in [&paths.root, &paths.cache, &paths.python] {
                std::fs::create_dir_all(directory)?;
                let test = directory.join(format!(".write-check-{}", Uuid::new_v4()));
                std::fs::write(&test, [])?;
                std::fs::remove_file(test)?;
            }
            let changed = self.active_paths.as_ref() != Some(&paths);
            settings.save(&self.preferences)?;
            Ok(changed)
        })();
        match result {
            Ok(true) => self.notice("restart_required"),
            Ok(false) => self.notice("options_saved"),
            Err(error) => self.error("operation_failed", format!("{error:#}")),
        }
    }
    pub fn copy(&mut self, text: String) {
        let result = arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text));
        match result {
            Ok(()) => self.notice("copied"),
            Err(error) => self.error("operation_failed", error.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installation_keeps_model_transfers_active_until_the_operation_stops() {
        use crate::{downloads::DownloadPhase, manager::Status};
        let dir = tempfile::tempdir().unwrap();
        let settings =
            Settings::from_file(dir.path().into(), dir.path().join("settings.json")).unwrap();
        let mut state = AppState::from_settings(Ok(settings));
        let instance = Instance::default();
        let id = instance.id;
        let transfer = ModelDownload::parse(
            r#"@ltgui-model:{"kind":"translation","source":"en","target":"es","downloaded":50,"total":100,"phase":"downloading"}"#,
        )
        .unwrap();
        let key = transfer.key();
        for (status, phase) in [
            (Status::Installing, DownloadPhase::Downloading),
            (Status::Starting, DownloadPhase::Downloading),
            (Status::UpdatingModels, DownloadPhase::Downloading),
            (Status::Stopped, DownloadPhase::Interrupted),
        ] {
            state.poll(PollUpdate {
                events: vec![Event::Snapshot(vec![View {
                    instance: instance.clone(),
                    status,
                    pid: None,
                }])],
                logs: vec![],
                downloads: Some(HashMap::from([(
                    id,
                    BTreeMap::from([(key.clone(), transfer.clone())]),
                )])),
            });
            assert_eq!(state.downloads[&id][&key].phase, phase);
            assert_eq!(state.downloads[&id][&key].downloaded, 50);
        }
    }
    #[test]
    fn audit_unrelated_background_error_does_not_discard_an_autosave() {
        let dir = tempfile::tempdir().unwrap();
        let settings =
            Settings::from_file(dir.path().into(), dir.path().join("settings.json")).unwrap();
        let mut state = AppState::from_settings(Ok(settings));
        let instance = Instance::default();
        let id = instance.id;
        state.saving = Some(instance.clone());
        state
            .pending_saves
            .lock()
            .unwrap()
            .insert(id, (instance, Instant::now()));
        state.poll(PollUpdate {
            events: vec![Event::Error(
                "Model update on another instance failed".into(),
            )],
            logs: vec![],
            downloads: None,
        });
        assert!(state.pending_saves.lock().unwrap().contains_key(&id));
        assert!(state.saving.is_some());
    }
    #[test]
    fn failed_autosave_keeps_draft_backs_off_and_an_edit_retries() {
        let dir = tempfile::tempdir().unwrap();
        let settings =
            Settings::from_file(dir.path().into(), dir.path().join("settings.json")).unwrap();
        let mut state = AppState::from_settings(Ok(settings));
        let instance = Instance::default();
        let id = instance.id;
        state.views.push(View {
            instance: instance.clone(),
            status: crate::manager::Status::Stopped,
            pid: None,
        });
        let mut draft = instance;
        draft.name = "Unsaved edit".into();
        let revision = Instant::now() - Duration::from_secs(1);
        state.editor = Some(draft.clone());
        state.saving = Some(draft.clone());
        state.saving_revision = Some(revision);
        state
            .pending_saves
            .lock()
            .unwrap()
            .insert(id, (draft, revision));
        state.poll(PollUpdate {
            events: vec![Event::SaveFailed {
                id,
                details: "disk full".into(),
            }],
            logs: vec![],
            downloads: None,
        });
        assert!(state.saving.is_none());
        assert!(state.has_unsaved(id));
        assert!(!state.autosave_due());
        assert_eq!(state.editor.as_ref().unwrap().name, "Unsaved edit");
        state.edit(id, |draft| draft.name = "New edit".into());
        assert!(state.failed_saves.is_empty());
        assert_eq!(state.pending_saves.lock().unwrap()[&id].0.name, "New edit");
    }
    #[test]
    fn removing_an_instance_discards_logs_and_save_feedback_even_with_queued_logs() {
        let dir = tempfile::tempdir().unwrap();
        let settings =
            Settings::from_file(dir.path().into(), dir.path().join("settings.json")).unwrap();
        let mut state = AppState::from_settings(Ok(settings));
        let id = Uuid::new_v4();
        state.logs.insert(id, VecDeque::from(["old".into()]));
        state.saved_at.insert(id, Instant::now());
        state
            .failed_saves
            .insert(id, (Instant::now(), Instant::now()));
        state.poll(PollUpdate {
            events: vec![Event::Snapshot(vec![])],
            logs: vec![crate::manager::Log {
                id,
                text: "late".into(),
            }],
            downloads: None,
        });
        assert!(
            state.logs.is_empty() && state.saved_at.is_empty() && state.failed_saves.is_empty()
        );
    }
    #[test]
    fn unrelated_save_acknowledgement_does_not_complete_another_draft() {
        let dir = tempfile::tempdir().unwrap();
        let settings =
            Settings::from_file(dir.path().into(), dir.path().join("settings.json")).unwrap();
        let mut state = AppState::from_settings(Ok(settings));
        let instance = Instance::default();
        let id = instance.id;
        state.saving = Some(instance.clone());
        state.editor = Some(instance.clone());
        state
            .pending_saves
            .lock()
            .unwrap()
            .insert(id, (instance, Instant::now()));
        state.poll(PollUpdate {
            events: vec![Event::Notice(Notice::Saved(Uuid::new_v4()))],
            logs: vec![],
            downloads: None,
        });
        assert!(state.saving.is_some());
        assert!(state.has_unsaved(id));
        assert_eq!(state.editor.unwrap().id, id);
    }
    #[test]
    fn follow_logs_persists_without_saving_unfinished_storage_edits() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let settings = Settings::from_file(dir.path().into(), file.clone()).unwrap();
        let mut state = AppState::from_settings(Ok(settings));
        state.preferences.data_dir = "unfinished/path".into();
        state.set_follow_logs(false);
        let restored = Settings::from_file(dir.path().into(), file).unwrap();
        assert!(!restored.preferences.follow_logs);
        assert_eq!(restored.preferences.data_dir, "data");
        assert_eq!(state.preferences.data_dir, "unfinished/path");
    }
    #[test]
    fn text_counter_options_persist_immediately_without_saving_unfinished_paths() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        std::fs::write(&file, r#"{"locale":"en","data_dir":"data"}"#).unwrap();
        let settings = Settings::from_file(dir.path().into(), file.clone()).unwrap();
        assert_eq!(
            settings.preferences.translator_counters,
            crate::settings::TranslatorCounters::default()
        );
        let mut state = AppState::from_settings(Ok(settings));
        state.preferences.data_dir = "unfinished/path".into();
        for (characters, words, labels) in [
            (false, true, true),
            (true, true, false),
            (false, false, true),
        ] {
            let counters = crate::settings::TranslatorCounters {
                characters,
                words,
                labels,
            };
            state.set_translator_counters(counters.clone());
            let restored = Settings::from_file(dir.path().into(), file.clone()).unwrap();
            assert_eq!(restored.preferences.translator_counters, counters);
            assert_eq!(restored.preferences.data_dir, "data");
            assert_eq!(state.preferences.data_dir, "unfinished/path");
        }
    }
    #[test]
    fn idle_poll_does_not_dirty_state_but_debounced_edits_and_feedback_expiry_do() {
        let dir = tempfile::tempdir().unwrap();
        let settings =
            Settings::from_file(dir.path().into(), dir.path().join("settings.json")).unwrap();
        let mut state = AppState::from_settings(Ok(settings));
        // Consume the actor's initial snapshot.
        {
            let backend = state.backend.lock().unwrap();
            let manager = backend.as_ref().unwrap();
            assert!(matches!(
                manager.events.recv_timeout(Duration::from_secs(3)).unwrap(),
                Event::Snapshot(_)
            ));
        }
        if let Some(update) = state.collect_poll() {
            state.poll(update);
        }
        assert!(state.collect_poll().is_none());
        let instance = Instance::default();
        let id = instance.id;
        state.views.push(View {
            instance: instance.clone(),
            status: crate::manager::Status::Stopped,
            pid: None,
        });
        let mut edited = instance;
        edited.name = "Edited".into();
        state
            .pending_saves
            .lock()
            .unwrap()
            .insert(id, (edited, Instant::now() - Duration::from_secs(1)));
        let update = state
            .collect_poll()
            .expect("mature valid edit must wake polling");
        state.poll(update);
        assert!(state.saving.is_some());
        state
            .saved_at
            .insert(id, Instant::now() - Duration::from_secs(3));
        let update = state
            .collect_poll()
            .expect("saved feedback must expire without unrelated events");
        state.poll(update);
        assert!(!state.saved_at.contains_key(&id));
    }
}
