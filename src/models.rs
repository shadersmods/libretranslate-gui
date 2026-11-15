use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::Path,
    sync::{LazyLock, Mutex},
    time::Duration,
};

const INDEX: &str = "https://raw.githubusercontent.com/argosopentech/argospm-index/main/index.json";
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Package {
    pub package_version: String,
    pub from_code: String,
    pub to_code: String,
    #[serde(default)]
    pub links: Vec<String>,
}
#[derive(Clone, Debug)]
pub struct ModelRow {
    pub from: String,
    pub to: String,
    pub installed: Option<String>,
    pub available: Option<String>,
    pub disk_bytes: Option<u64>,
    pub download_url: Option<String>,
    pub update: bool,
}
pub struct ModelSnapshot {
    pub rows: Vec<ModelRow>,
    pub cached: bool,
    pub storage_bytes: Option<u64>,
}
fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(10))
        .user_agent("LibreTranslate-GUI/0.2")
        .build()?)
}
fn read_index(cache: &Path, refresh: bool) -> Result<(Vec<Package>, bool)> {
    let file = cache.join("gui-model-index.json");
    let fresh = !refresh
        && fs::metadata(&file)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age < Duration::from_secs(3600));
    if fresh {
        if let Ok(bytes) = fs::read(&file) {
            if let Ok(index) = serde_json::from_slice(&bytes) {
                return Ok((index, false));
            }
        }
    }
    let fetched = (|| -> Result<Vec<Package>> {
        let bytes = client()?.get(INDEX).send()?.error_for_status()?.bytes()?;
        let index: Vec<Package> = serde_json::from_slice(&bytes)?;
        fs::create_dir_all(cache)?;
        let temporary = cache.join(format!("gui-model-index-{}.tmp", uuid::Uuid::new_v4()));
        fs::write(&temporary, &bytes)?;
        fs::rename(&temporary, &file)?;
        Ok(index)
    })();
    match fetched {
        Ok(index) => Ok((index, false)),
        Err(_) => {
            let index = fs::read(&file)
                .ok()
                .and_then(|bytes| serde_json::from_slice(&bytes).ok())
                .unwrap_or_else(|| {
                    serde_json::from_str(include_str!("../assets/model-index.json"))
                        .expect("Invalid bundled model catalog")
                });
            Ok((index, true))
        }
    }
}
pub fn newer(candidate: &str, installed: &str) -> bool {
    let parse = |value: &str| {
        value
            .split('.')
            .map(str::parse::<u64>)
            .collect::<std::result::Result<Vec<_>, _>>()
    };
    let (Ok(mut a), Ok(mut b)) = (parse(candidate), parse(installed)) else {
        return false;
    };
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}
fn directory_bytes(path: &Path) -> Option<u64> {
    let mut total = 0;
    let entries = fs::read_dir(path).ok()?;
    for entry in entries {
        let entry = entry.ok()?;
        let kind = entry.file_type().ok()?;
        if kind.is_symlink() {
            continue;
        }
        total += if kind.is_dir() {
            directory_bytes(&entry.path())?
        } else {
            entry.metadata().ok()?.len()
        };
    }
    Some(total)
}
pub fn inspect(
    root: &Path,
    cache: &Path,
    id: uuid::Uuid,
    languages: &str,
    refresh: bool,
) -> Result<ModelSnapshot> {
    let (index, cached) = read_index(cache, refresh)?;
    let codes = languages.split(',').collect::<Vec<_>>();
    let mut rows: BTreeMap<(String, String), ModelRow> = BTreeMap::new();
    for package in index
        .into_iter()
        .filter(|p| codes.contains(&p.from_code.as_str()) && codes.contains(&p.to_code.as_str()))
    {
        let url = package
            .links
            .into_iter()
            .find(|url| url.starts_with("https://"));
        rows.insert(
            (package.from_code.clone(), package.to_code.clone()),
            ModelRow {
                from: package.from_code,
                to: package.to_code,
                installed: None,
                available: Some(package.package_version),
                disk_bytes: None,
                download_url: url,
                update: false,
            },
        );
    }
    let packages = crate::runtime::instance_dir(root, id).join("packages");
    if let Ok(entries) = fs::read_dir(packages) {
        for entry in entries.flatten() {
            if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            let Ok(bytes) = fs::read(entry.path().join("metadata.json")) else {
                continue;
            };
            let Ok(package) = serde_json::from_slice::<Package>(&bytes) else {
                continue;
            };
            if !codes.contains(&package.from_code.as_str())
                || !codes.contains(&package.to_code.as_str())
            {
                continue;
            }
            let row = rows
                .entry((package.from_code.clone(), package.to_code.clone()))
                .or_insert_with(|| ModelRow {
                    from: package.from_code,
                    to: package.to_code,
                    installed: None,
                    available: None,
                    disk_bytes: None,
                    download_url: None,
                    update: false,
                });
            if row
                .installed
                .as_ref()
                .is_none_or(|old| newer(&package.package_version, old))
            {
                row.disk_bytes = directory_bytes(&entry.path());
                row.installed = Some(package.package_version);
            }
            row.update = match (&row.available, &row.installed) {
                (Some(new), Some(old)) => newer(new, old),
                _ => false,
            };
        }
    }
    Ok(ModelSnapshot {
        rows: rows.into_values().collect(),
        cached,
        storage_bytes: directory_bytes(&crate::runtime::instance_dir(root, id)),
    })
}
static SIZES: LazyLock<Mutex<HashMap<String, Option<u64>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static SIZE_SLOTS: LazyLock<tokio::sync::Semaphore> =
    LazyLock::new(|| tokio::sync::Semaphore::new(4));
pub fn clear_sizes() {
    SIZES.lock().unwrap().clear();
}
pub async fn download_size(url: String) -> Option<u64> {
    if let Some(size) = SIZES.lock().unwrap().get(&url).copied() {
        return size;
    }
    let slot = SIZE_SLOTS.acquire().await.ok()?;
    tokio::task::spawn_blocking(move || {
        // The network task retains its slot even when a tab is closed mid-request.
        let _slot = slot;
        let result = (|| -> Result<Option<u64>> {
            let client = client()?;
            if let Ok(head) = client.head(&url).send() {
                if head.status().is_success() {
                    if let Some(size) = head
                        .headers()
                        .get(reqwest::header::CONTENT_LENGTH)
                        .and_then(|value| value.to_str().ok())
                        .and_then(|value| value.parse::<u64>().ok())
                        .filter(|size| *size > 0)
                    {
                        return Ok(Some(size));
                    }
                }
            }
            let response = client
                .get(&url)
                .header("Range", "bytes=0-0")
                .send()?
                .error_for_status()?;
            if let Some(range) = response
                .headers()
                .get("Content-Range")
                .and_then(|value| value.to_str().ok())
            {
                return Ok(range
                    .rsplit('/')
                    .next()
                    .and_then(|value| value.parse().ok()));
            }
            Ok(response.content_length())
        })()
        .ok()
        .flatten();
        SIZES.lock().unwrap().insert(url, result);
        result
    })
    .await
    .ok()
    .flatten()
}
pub fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 * 1024 {
        format!("{:.1} GiB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compares_model_versions_without_lexical_ordering() {
        assert!(newer("1.10", "1.9"));
        assert!(newer("1.9.6", "1.9"));
        assert!(!newer("1.9.0", "1.9"));
        assert!(!newer("preview", "1.9"));
    }
    #[test]
    fn bundled_catalog_has_real_pairs_and_https_downloads() {
        let packages: Vec<Package> =
            serde_json::from_str(include_str!("../assets/model-index.json")).unwrap();
        assert!(
            packages
                .iter()
                .any(|p| p.from_code == "en" && p.to_code == "ro")
        );
        assert!(
            packages
                .iter()
                .all(|p| p.links.iter().any(|url| url.starts_with("https://")))
        );
    }
}
