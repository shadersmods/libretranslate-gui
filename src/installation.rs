//! MSI program files and user data have separate lifetimes. ZIP builds stay portable.
use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};

const MARKER: &str = "libretranslate-gui.install.json";

pub fn is_msi_at(program: &Path) -> bool {
    if !std::fs::metadata(program.join(MARKER))
        .is_ok_and(|meta| meta.is_file() && meta.len() <= 256)
    {
        return false;
    }
    std::fs::read(program.join(MARKER))
        .ok()
        .filter(|bytes| bytes.len() <= 256)
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|value| value["schema"] == 1 && value["kind"] == "msi")
}

pub fn is_msi() -> bool {
    cfg!(windows)
        && std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(is_msi_at))
            .unwrap_or(false)
}

pub fn settings_base(program: &Path, local_app_data: Option<PathBuf>) -> Result<PathBuf> {
    if !cfg!(windows) || !is_msi_at(program) {
        return Ok(program.to_path_buf());
    }
    let local =
        local_app_data.context("Windows did not provide a local application data folder")?;
    ensure!(
        local.is_absolute(),
        "The local application data folder must be absolute"
    );
    Ok(local.join("LibreTranslate GUI"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installed_data_is_outside_program_files_and_portable_data_stays_beside_exe() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("Programs/LibreTranslate GUI");
        std::fs::create_dir_all(&program).unwrap();
        assert_eq!(settings_base(&program, None).unwrap(), program);
        std::fs::write(program.join(MARKER), br#"{"schema":1,"kind":"msi"}"#).unwrap();
        assert!(is_msi_at(&program));
        let local = dir.path().join("Local");
        let base = settings_base(&program, Some(local.clone())).unwrap();
        if cfg!(windows) {
            assert_eq!(base, local.join("LibreTranslate GUI"));
            assert!(!base.starts_with(&program));
            assert!(settings_base(&program, None).is_err());
            assert!(settings_base(&program, Some("relative".into())).is_err());
            let file = base.join("libretranslate-desktop.json");
            let mut settings =
                crate::settings::Settings::from_file(base.clone(), file.clone()).unwrap();
            settings
                .save(&crate::settings::Preferences::default())
                .unwrap();
            let data = settings.paths(&settings.preferences).unwrap().root;
            std::fs::create_dir_all(&data).unwrap();
            std::fs::write(data.join("instances.json"), "keep instances").unwrap();
            std::fs::remove_dir_all(&program).unwrap();
            assert!(file.is_file());
            assert_eq!(
                std::fs::read_to_string(data.join("instances.json")).unwrap(),
                "keep instances"
            );
            std::fs::create_dir_all(&program).unwrap();
        } else {
            assert_eq!(base, program);
        }
        std::fs::write(program.join(MARKER), br#"{"schema":2,"kind":"msi"}"#).unwrap();
        assert!(!is_msi_at(&program));
    }
}
