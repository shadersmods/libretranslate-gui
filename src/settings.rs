use crate::i18n::Locale;
use crate::paths::{same_or_child, same_path};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
    #[default]
    System,
}
impl Theme {
    pub fn code(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::System => "system",
        }
    }
    pub fn from_code(code: &str) -> Self {
        match code {
            "light" => Self::Light,
            "dark" => Self::Dark,
            _ => Self::System,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Preferences {
    pub locale: Locale,
    pub theme: Theme,
    pub data_dir: String,
    pub cache_dir: String,
    pub python_dir: String,
    pub follow_logs: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            locale: Locale::system(),
            theme: Theme::System,
            data_dir: "data".into(),
            cache_dir: String::new(),
            python_dir: String::new(),
            follow_logs: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoragePaths {
    pub root: PathBuf,
    pub cache: PathBuf,
    pub python: PathBuf,
}
impl StoragePaths {
    pub fn new(root: PathBuf) -> Self {
        Self {
            cache: root.join("cache"),
            python: root.join("python"),
            root,
        }
    }
}

pub struct Settings {
    pub file: PathBuf,
    pub base: PathBuf,
    pub preferences: Preferences,
}
impl Settings {
    pub fn load() -> Result<Self> {
        let program = std::env::current_exe()?
            .parent()
            .context("The executable has no parent folder")?
            .to_path_buf();
        let base = crate::installation::settings_base(
            &program,
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        )?;
        let file = std::env::var_os("LIBRETRANSLATE_GUI_SETTINGS_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| base.join("libretranslate-desktop.json"));
        Self::from_file(base, file)
    }
    pub fn from_file(base: PathBuf, file: PathBuf) -> Result<Self> {
        let preferences = if file.exists() {
            serde_json::from_slice(&fs::read(&file)?).with_context(|| {
                format!(
                    "Could not read {}. Restore a valid settings file.",
                    file.display()
                )
            })?
        } else {
            Preferences::default()
        };
        Ok(Self {
            base,
            file,
            preferences,
        })
    }
    pub fn paths(&self, preferences: &Preferences) -> Result<StoragePaths> {
        ensure!(
            !preferences.data_dir.trim().is_empty(),
            "Choose a data folder."
        );
        let root = std::env::var_os("LIBRETRANSLATE_GUI_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| resolve(&self.base, &preferences.data_dir));
        ensure!(
            root.is_absolute(),
            "Use an absolute data-directory environment override."
        );
        let root = normalize(&root);
        let cache = if preferences.cache_dir.trim().is_empty() {
            root.join("cache")
        } else {
            resolve(&self.base, &preferences.cache_dir)
        };
        let python = if preferences.python_dir.trim().is_empty() {
            root.join("python")
        } else {
            resolve(&self.base, &preferences.python_dir)
        };
        let root_identity = path_identity(&root);
        let cache_identity = path_identity(&cache);
        let python_identity = path_identity(&python);
        ensure!(
            !same_path(&root_identity, &cache_identity)
                && !same_path(&root_identity, &python_identity)
                && !same_path(&cache_identity, &python_identity),
            "Use different folders for instance data, download cache, and Python."
        );
        // No shared runtime may live inside an instance, which can be deleted separately.
        ensure!(
            ["instances", "instance-removals"].iter().all(|name| {
                let reserved = root_identity.join(name);
                !same_or_child(&cache_identity, &reserved)
                    && !same_or_child(&python_identity, &reserved)
            }),
            "Shared paths cannot be inside instance-owned folders."
        );
        let mut paths = StoragePaths::new(root);
        paths.cache = cache;
        paths.python = python;
        Ok(paths)
    }
    pub fn save(&mut self, preferences: &Preferences) -> Result<()> {
        self.paths(preferences)?;
        if let Some(parent) = self.file.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = self.file.with_extension("json.tmp");
        let mut file = fs::File::create(&temporary)
            .context("Could not save settings. Choose a writable location with LIBRETRANSLATE_GUI_SETTINGS_FILE.")?;
        use std::io::Write;
        file.write_all(&serde_json::to_vec_pretty(preferences)?)?;
        file.sync_all()?;
        fs::rename(temporary, &self.file)?;
        self.preferences = preferences.clone();
        Ok(())
    }
}

fn path_identity(path: &Path) -> PathBuf {
    let mut existing = path.to_path_buf();
    let mut suffix = Vec::new();
    loop {
        if let Ok(mut canonical) = dunce::canonicalize(&existing) {
            for part in suffix.iter().rev() {
                canonical.push(part);
            }
            return identity_prefix(&canonical);
        }
        let Some(part) = existing.file_name().map(|name| name.to_os_string()) else {
            return identity_prefix(path);
        };
        suffix.push(part);
        if !existing.pop() {
            return identity_prefix(path);
        }
    }
}
// Canonical long paths can keep an extended prefix. Ignore that prefix only
// for identity checks, so an existing long child still matches its shorter
// reserved ancestor. Actual I/O keeps the original path and literal suffixes.
fn identity_prefix(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut parts = path.components();
        let mut identity = match parts.next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::VerbatimDisk(drive) => PathBuf::from(format!("{}:", drive as char)),
                Prefix::VerbatimUNC(server, share) => {
                    let mut root = PathBuf::from(r"\\");
                    root.push(server);
                    root.push(share);
                    root
                }
                _ => return path.to_path_buf(),
            },
            _ => return path.to_path_buf(),
        };
        for part in parts {
            identity.push(part.as_os_str());
        }
        identity
    }
    #[cfg(not(windows))]
    {
        path.to_path_buf()
    }
}
fn resolve(base: &Path, input: &str) -> PathBuf {
    let path = Path::new(input.trim());
    // Lexical normalization keeps defaults usable before folders exist.
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    };
    normalize(&joined)
}
#[cfg(windows)]
fn normalize(path: &Path) -> PathBuf {
    // Use Win32's own lexical rules before folders exist. In particular,
    // intermediate spaces and literal suffixes in verbatim paths are retained.
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}
#[cfg(not(windows))]
fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn portable_defaults_and_custom_paths_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        let mut settings = Settings::from_file(dir.path().into(), file.clone()).unwrap();
        let defaults = settings.paths(&settings.preferences).unwrap();
        assert_eq!(defaults.root, dir.path().join("data"));
        let preferences = Preferences {
            locale: Locale::Ro,
            cache_dir: "downloads".into(),
            python_dir: "runtimes/python".into(),
            ..Default::default()
        };
        settings.save(&preferences).unwrap();
        let loaded = Settings::from_file(dir.path().into(), file).unwrap();
        assert_eq!(loaded.preferences, preferences);
        assert_eq!(
            loaded.paths(&preferences).unwrap().cache,
            dir.path().join("downloads")
        );
    }
    #[test]
    fn rejects_overlapping_and_instance_owned_shared_paths() {
        let dir = tempfile::tempdir().unwrap();
        let settings =
            Settings::from_file(dir.path().into(), dir.path().join("settings.json")).unwrap();
        assert!(
            settings
                .paths(&Preferences {
                    cache_dir: "data/./instances/one/cache".into(),
                    ..Default::default()
                })
                .is_err()
        );
        for path in ["data/instance-removals", "data/instance-removals/one/cache"] {
            assert!(
                settings
                    .paths(&Preferences {
                        cache_dir: path.into(),
                        ..Default::default()
                    })
                    .is_err()
            );
            assert!(
                settings
                    .paths(&Preferences {
                        python_dir: path.into(),
                        ..Default::default()
                    })
                    .is_err()
            );
        }
        #[cfg(windows)]
        for path in [
            "DATA/Instances/one/cache",
            "DATA/INSTANCE-REMOVALS/cache",
            "DATA",
        ] {
            assert!(
                settings
                    .paths(&Preferences {
                        cache_dir: path.into(),
                        ..Default::default()
                    })
                    .is_err()
            );
        }
        assert!(
            settings
                .paths(&Preferences {
                    cache_dir: "data".into(),
                    ..Default::default()
                })
                .is_err()
        );
    }
    #[cfg(windows)]
    #[test]
    fn rejects_reserved_paths_with_win32_suffix_aliases_before_creation() {
        let dir = tempfile::tempdir().unwrap();
        let settings =
            Settings::from_file(dir.path().into(), dir.path().join("settings.json")).unwrap();
        for path in [
            "data/instances./one/cache",
            "data/instance-removals./one/cache",
            "data./instances/one/cache",
            "data/instances...",
            "data/instance-removals...",
        ] {
            for python in [false, true] {
                let mut preferences = Preferences::default();
                if python {
                    preferences.python_dir = path.into();
                } else {
                    preferences.cache_dir = path.into();
                }
                assert!(settings.paths(&preferences).is_err(), "accepted {path}");
            }
        }
    }
    #[cfg(windows)]
    #[test]
    fn rejects_unicode_case_aliases_of_missing_storage_folders() {
        let dir = tempfile::tempdir().unwrap();
        let settings =
            Settings::from_file(dir.path().into(), dir.path().join("settings.json")).unwrap();
        for path in ["données", "DONNÉES/instances/one/cache"] {
            let preferences = Preferences {
                data_dir: "Données".into(),
                cache_dir: path.into(),
                ..Default::default()
            };
            assert!(settings.paths(&preferences).is_err(), "accepted {path}");
        }
    }
    #[cfg(windows)]
    #[test]
    fn extended_paths_keep_literal_suffixes_but_match_reserved_ancestors() {
        let dir = tempfile::tempdir().unwrap();
        let settings =
            Settings::from_file(dir.path().into(), dir.path().join("settings.json")).unwrap();
        let reserved = dir.path().join("data/instances");
        std::fs::create_dir_all(&reserved).unwrap();
        let mut long = std::fs::canonicalize(&reserved).unwrap();
        for _ in 0..8 {
            long.push("long-directory-component-for-storage");
        }
        std::fs::create_dir_all(&long).unwrap();
        assert!(
            settings
                .paths(&Preferences {
                    cache_dir: long.to_str().unwrap().into(),
                    ..Default::default()
                })
                .is_err()
        );

        let literal = Path::new(r"\\?\C:\data\instances.\cache");
        assert_eq!(normalize(literal), literal);
        assert_ne!(
            identity_prefix(literal),
            Path::new(r"C:\data\instances\cache")
        );
        let spaces = Path::new(r"C:\data\instances \cache");
        assert_eq!(normalize(spaces), spaces);
        assert_eq!(
            normalize(Path::new(r"C:\data\instances... ")),
            Path::new(r"C:\data\instances")
        );
    }
}
