use serde::Deserialize;
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

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
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
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DownloadKind {
    Translation,
    Sentence,
}
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ModelDownload {
    pub kind: DownloadKind,
    pub source: String,
    pub target: Option<String>,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub phase: DownloadPhase,
}
impl ModelDownload {
    pub fn parse(line: &str) -> Option<Self> {
        let progress: Self = serde_json::from_str(line.strip_prefix(PREFIX)?).ok()?;
        let valid_code = |code: &str| {
            !code.is_empty()
                && code.len() <= 16
                && code.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
        };
        if !valid_code(&progress.source)
            || progress
                .target
                .as_deref()
                .is_some_and(|code| !valid_code(code))
            || (progress.kind == DownloadKind::Translation && progress.target.is_none())
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
    pub fn percent(&self) -> Option<f64> {
        self.total
            .map(|total| self.downloaded as f64 / total as f64 * 100.0)
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
