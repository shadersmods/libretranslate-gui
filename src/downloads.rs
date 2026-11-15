use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
};
use uuid::Uuid;

pub const PREFIX: &str = "@ltgui-model:";
pub type SharedDownloads = Arc<Mutex<HashMap<Uuid, BTreeMap<String, ModelDownload>>>>;

pub fn dismiss_finished(downloads: &SharedDownloads, item: Option<(Uuid, &str)>) {
    let mut downloads = downloads.lock().unwrap();
    for (id, entries) in downloads.iter_mut() {
        entries.retain(|key, download| {
            download.phase.active()
                || item.is_some_and(|(selected_id, selected_key)| {
                    selected_id != *id || selected_key != key
                })
        });
    }
    downloads.retain(|_, entries| !entries.is_empty());
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadPhase {
    Queued,
    Downloading,
    Validating,
    Complete,
    Failed,
    Interrupted,
}
impl DownloadPhase {
    pub fn active(self) -> bool {
        matches!(self, Self::Queued | Self::Downloading | Self::Validating)
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "download_queued",
            Self::Downloading => "download_active",
            Self::Validating => "download_validating",
            Self::Complete => "download_complete",
            Self::Failed => "download_failed",
            Self::Interrupted => "download_interrupted",
        }
    }
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadKind {
    Translation,
    Sentence,
    Runtime,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ModelDownload {
    pub kind: DownloadKind,
    pub source: String,
    pub target: Option<String>,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub phase: DownloadPhase,
    /// uv reports rounded package sizes, not transferred byte counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_hint: Option<String>,
}
impl ModelDownload {
    pub fn parse(line: &str) -> Option<Self> {
        let progress: Self = serde_json::from_str(line.strip_prefix(PREFIX)?).ok()?;
        let valid_code = |code: &str| {
            !code.is_empty()
                && code.len() <= 16
                && code.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        };
        let runtime = progress.kind == DownloadKind::Runtime;
        if !(if runtime {
            valid_runtime_name(&progress.source)
        } else {
            valid_code(&progress.source)
        }) || progress
            .target
            .as_deref()
            .is_some_and(|code| !valid_code(code))
            || (progress.kind == DownloadKind::Translation && progress.target.is_none())
            || (runtime && progress.target.is_some())
            || progress.size_hint.as_ref().is_some_and(|hint| {
                !runtime
                    || hint.len() > 32
                    || !hint
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b' '))
            })
            || progress.total == Some(0)
            || progress
                .total
                .is_some_and(|total| progress.downloaded > total)
        {
            return None;
        }
        Some(progress)
    }
    pub fn key(&self) -> String {
        format!(
            "{:?}:{}:{}",
            self.kind,
            self.source,
            self.target.as_deref().unwrap_or_default()
        )
    }
    pub fn runtime(
        source: &str,
        downloaded: u64,
        total: Option<u64>,
        phase: DownloadPhase,
    ) -> Self {
        Self {
            kind: DownloadKind::Runtime,
            source: source.into(),
            target: None,
            downloaded,
            total,
            phase,
            size_hint: None,
        }
    }
    pub fn message(&self) -> String {
        format!(
            "{PREFIX}{}",
            serde_json::to_string(self).expect("Download progress is serializable")
        )
    }
    pub fn percent(&self) -> Option<f64> {
        self.total
            .map(|total| self.downloaded as f64 / total as f64 * 100.0)
    }
}
fn valid_runtime_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 160
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'))
}

#[derive(Default)]
pub struct RuntimeTransfers {
    entries: BTreeMap<String, ModelDownload>,
}
impl RuntimeTransfers {
    pub fn observe_uv(&mut self, line: &str) -> Option<ModelDownload> {
        let line = line.trim();
        let (description, phase) = if let Some(rest) = line.strip_prefix("Downloading ") {
            (rest, DownloadPhase::Downloading)
        } else {
            (line.strip_prefix("Downloaded ")?, DownloadPhase::Complete)
        };
        let name = description.split_whitespace().next()?;
        if !valid_runtime_name(name) {
            return None;
        }
        let mut transfer = self
            .entries
            .get(name)
            .cloned()
            .unwrap_or_else(|| ModelDownload::runtime(name, 0, None, phase));
        transfer.phase = phase;
        if let Some(size) = description
            .strip_prefix(name)?
            .trim()
            .strip_prefix('(')
            .and_then(|rest| rest.split(')').next())
            && size.len() <= 32
            && size.bytes().any(|byte| byte.is_ascii_digit())
            && size
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b' '))
        {
            transfer.size_hint = Some(size.into());
        }
        if self.entries.len() >= 256 && !self.entries.contains_key(name) {
            return None;
        }
        self.entries.insert(name.into(), transfer.clone());
        Some(transfer)
    }
    pub fn finish(&mut self, phase: DownloadPhase) -> Vec<ModelDownload> {
        self.entries
            .values_mut()
            .filter(|transfer| transfer.phase.active())
            .map(|transfer| {
                transfer.phase = phase;
                transfer.clone()
            })
            .collect()
    }
}
pub fn interrupt(downloads: &SharedDownloads, id: Uuid) {
    if let Some(models) = downloads.lock().unwrap().get_mut(&id) {
        for model in models.values_mut().filter(|model| model.phase.active()) {
            model.phase = DownloadPhase::Interrupted;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn runtime_tasks_keep_reported_sizes_separate_from_measured_bytes_and_finish_on_command_failure()
     {
        let mut transfers = RuntimeTransfers::default();
        let python = transfers
            .observe_uv("Downloading cpython-3.11.18-windows-x86_64-none (21.5MiB)")
            .unwrap();
        assert_eq!(python.kind, DownloadKind::Runtime);
        assert_eq!(python.size_hint.as_deref(), Some("21.5MiB"));
        assert_eq!(python.total, None);
        assert_eq!(python.percent(), None);
        assert_eq!(ModelDownload::parse(&python.message()), Some(python));
        let cuda = transfers
            .observe_uv(" Downloading nvidia-cublas-cu12 (541.3MiB)")
            .unwrap();
        assert_eq!(cuda.source, "nvidia-cublas-cu12");
        let complete = transfers
            .observe_uv("Downloaded cpython-3.11.18-windows-x86_64-none")
            .unwrap();
        assert_eq!(complete.size_hint.as_deref(), Some("21.5MiB"));
        assert_eq!(complete.phase, DownloadPhase::Complete);
        let failed = transfers.finish(DownloadPhase::Failed);
        assert_eq!(failed.len(), 1);
        assert_eq!(failed[0].source, "nvidia-cublas-cu12");
        assert_eq!(failed[0].phase, DownloadPhase::Failed);
        assert!(transfers.finish(DownloadPhase::Interrupted).is_empty());
        transfers
            .observe_uv("Downloading nvidia-cuda-runtime-cu12 (3.0MiB)")
            .unwrap();
        let stopped = transfers.finish(DownloadPhase::Interrupted);
        assert_eq!(stopped.len(), 1);
        assert_eq!(stopped[0].phase, DownloadPhase::Interrupted);
        assert!(
            transfers
                .observe_uv("Downloading https://example.com/package.whl")
                .is_none()
        );
        assert!(transfers.observe_uv("Downloading ../private").is_none());
        assert!(transfers.observe_uv("Resolved 66 packages in 2s").is_none());
    }
    #[test]
    fn parses_real_sizes_unknown_totals_and_rejects_invalid_messages() {
        let known = ModelDownload::parse(r#"@ltgui-model:{"kind":"translation","source":"en","target":"ro","downloaded":1048576,"total":2097152,"phase":"downloading"}"#).unwrap();
        assert_eq!(known.percent(), Some(50.0));
        let unknown = ModelDownload::parse(r#"@ltgui-model:{"kind":"sentence","source":"en","downloaded":4096,"total":null,"phase":"downloading"}"#).unwrap();
        assert_eq!(unknown.percent(), None);
        assert!(ModelDownload::parse("Downloading model: en").is_none());
        assert!(ModelDownload::parse(r#"@ltgui-model:{"kind":"translation","source":"en","target":"ro","downloaded":3,"total":2,"phase":"downloading"}"#).is_none());
        assert!(ModelDownload::parse(r#"@ltgui-model:{"kind":"translation","source":"../en","target":"ro","downloaded":0,"phase":"downloading"}"#).is_none());
    }
    #[test]
    fn stopping_retains_byte_counts_and_completed_downloads() {
        let downloads = SharedDownloads::default();
        let id = Uuid::new_v4();
        let mut entry = ModelDownload::parse(r#"@ltgui-model:{"kind":"sentence","source":"en","downloaded":4096,"phase":"downloading"}"#).unwrap();
        downloads
            .lock()
            .unwrap()
            .entry(id)
            .or_default()
            .insert(entry.key(), entry.clone());
        entry.source = "ro".into();
        entry.phase = DownloadPhase::Complete;
        downloads
            .lock()
            .unwrap()
            .get_mut(&id)
            .unwrap()
            .insert(entry.key(), entry);
        interrupt(&downloads, id);
        let entries = downloads.lock().unwrap();
        let values = &entries[&id];
        assert!(
            values
                .values()
                .any(|value| value.phase == DownloadPhase::Interrupted && value.downloaded == 4096)
        );
        assert!(
            values
                .values()
                .any(|value| value.phase == DownloadPhase::Complete)
        );
    }
}

#[cfg(test)]
mod history_tests {
    use super::*;
    #[test]
    fn dismissing_history_preserves_active_work_and_other_instances() {
        let shared = SharedDownloads::default();
        let first = Uuid::new_v4();
        let second = Uuid::new_v4();
        let completed=ModelDownload::parse(r#"@ltgui-model:{"kind":"sentence","source":"en","downloaded":8,"total":8,"phase":"complete"}"#).unwrap();
        let active=ModelDownload::parse(r#"@ltgui-model:{"kind":"sentence","source":"ro","downloaded":2,"total":8,"phase":"downloading"}"#).unwrap();
        {
            let mut data = shared.lock().unwrap();
            data.entry(first)
                .or_default()
                .insert(completed.key(), completed.clone());
            data.entry(first)
                .or_default()
                .insert(active.key(), active.clone());
            data.entry(second)
                .or_default()
                .insert(completed.key(), completed.clone());
        }
        dismiss_finished(&shared, Some((first, &active.key())));
        assert_eq!(shared.lock().unwrap()[&first].len(), 2);
        dismiss_finished(&shared, Some((first, &completed.key())));
        assert_eq!(shared.lock().unwrap()[&first].len(), 1);
        assert_eq!(shared.lock().unwrap()[&second].len(), 1);
        dismiss_finished(&shared, None);
        let data = shared.lock().unwrap();
        assert_eq!(data.len(), 1);
        assert_eq!(data[&first].values().next().unwrap(), &active);
    }
}
