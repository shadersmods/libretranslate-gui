use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path, time::Duration};

pub const DEFAULT_VERSION: &str = "1.9.6";
pub fn version_tuple(value: &str) -> Option<(u32, u32, u32)> {
    let parts = value
        .split('.')
        .map(str::parse::<u32>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .ok()?;
    if parts.len() != 3 {
        return None;
    }
    Some((parts[0], parts[1], parts[2]))
}
pub fn supported(value: &str) -> bool {
    value.len() <= 32 && version_tuple(value).is_some_and(|version| version >= (1, 9, 1))
}
pub fn newer(candidate: &str, installed: &str) -> bool {
    matches!((version_tuple(candidate),version_tuple(installed)),(Some(new),Some(old)) if new>old)
}
pub fn default_version() -> String {
    crate::runtime::LIBRETRANSLATE_VERSION.into()
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct ReleaseCatalog {
    pub versions: Vec<String>,
    pub latest: String,
    #[serde(default)]
    pub offline: bool,
}
impl ReleaseCatalog {
    pub fn fallback() -> Self {
        Self {
            versions: (1..=6).rev().map(|patch| format!("1.9.{patch}")).collect(),
            latest: DEFAULT_VERSION.into(),
            offline: true,
        }
    }
    fn parse(value: serde_json::Value) -> Result<Self> {
        let mut versions = value["releases"]
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(version, files)| {
                supported(version)
                    && files.as_array().is_some_and(|files| {
                        files
                            .iter()
                            .any(|file| file["yanked"].as_bool() == Some(false))
                    })
            })
            .map(|(version, _)| version.clone())
            .collect::<Vec<_>>();
        versions.sort_by_key(|version| std::cmp::Reverse(version_tuple(version)));
        ensure!(
            !versions.is_empty(),
            "No supported stable LibreTranslate releases were returned by PyPI."
        );
        Ok(Self {
            latest: versions[0].clone(),
            versions,
            offline: false,
        })
    }
}
pub fn check(cache: &Path, force: bool) -> Result<ReleaseCatalog> {
    let file = cache.join("libretranslate-releases.json");
    let cached = fs::read(&file)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<ReleaseCatalog>(&bytes).ok())
        .filter(|catalog| {
            !catalog.versions.is_empty()
                && catalog.versions.iter().all(|version| supported(version))
                && catalog.versions.contains(&catalog.latest)
        });
    if !force
        && fs::metadata(&file)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|time| time.elapsed().ok())
            .is_some_and(|age| age < Duration::from_secs(3600))
    {
        if let Some(catalog) = &cached {
            return Ok(catalog.clone());
        }
    }
    let result = (|| -> Result<ReleaseCatalog> {
        let response = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .user_agent("LibreTranslate-GUI")
            .build()?
            .get("https://pypi.org/pypi/libretranslate/json")
            .send()?
            .error_for_status()?;
        use std::io::Read;
        let mut bytes = Vec::new();
        response.take(4 * 1024 * 1024).read_to_end(&mut bytes)?;
        let catalog = ReleaseCatalog::parse(serde_json::from_slice(&bytes)?)?;
        fs::create_dir_all(cache)?;
        let temp = cache.join(format!("releases-{}.tmp", uuid::Uuid::new_v4()));
        fs::write(&temp, serde_json::to_vec(&catalog)?)?;
        fs::rename(temp, file)?;
        Ok(catalog)
    })();
    Ok(result.unwrap_or_else(|_| {
        let mut catalog = cached.unwrap_or_else(ReleaseCatalog::fallback);
        catalog.offline = true;
        catalog
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filters_prereleases_yanked_versions_and_sorts_numerically() {
        let catalog=ReleaseCatalog::parse(serde_json::json!({"releases":{"1.9.5":[{"yanked":false}],"1.10.0":[{"yanked":false}],"1.9.6rc1":[{"yanked":false}],"2.0.0":[{"yanked":true}],"1.8.4":[{"yanked":false}]}})).unwrap();
        assert_eq!(catalog.versions, vec!["1.10.0", "1.9.5"]);
        assert_eq!(catalog.latest, "1.10.0");
        assert!(newer(&catalog.latest, "1.9.6"));
        assert!(!supported("1.9.6 --help"));
        assert!(!supported("1.8.4"));
    }
}
