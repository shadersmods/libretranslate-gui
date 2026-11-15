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
        && !is_msix()
        && std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(is_msi_at))
            .unwrap_or(false)
}

fn msix_family() -> Option<&'static str> {
    #[cfg(windows)]
    {
        static FAMILY: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
        FAMILY
            .get_or_init(|| unsafe {
                use windows_sys::Win32::{
                    Foundation::ERROR_INSUFFICIENT_BUFFER,
                    Storage::Packaging::Appx::GetCurrentPackageFamilyName,
                };
                let mut length = 0;
                if GetCurrentPackageFamilyName(&mut length, std::ptr::null_mut())
                    != ERROR_INSUFFICIENT_BUFFER
                    || length > 512
                {
                    return None;
                }
                let mut name = vec![0u16; length as usize];
                if GetCurrentPackageFamilyName(&mut length, name.as_mut_ptr()) != 0 {
                    return None;
                }
                String::from_utf16(&name[..length.saturating_sub(1) as usize])
                    .ok()
                    .filter(|family| {
                        family.starts_with("shadersmods.LibreTranslateGUI_")
                            || family.starts_with("shadersmods.LibreTranslateGUI.Unsigned_")
                    })
            })
            .as_deref()
    }
    #[cfg(not(windows))]
    {
        None
    }
}
pub fn is_msix() -> bool {
    msix_family().is_some()
}
pub fn is_unsigned_msix() -> bool {
    msix_family()
        .is_some_and(|family| family.starts_with("shadersmods.LibreTranslateGUI.Unsigned_"))
}

pub fn settings_base(program: &Path, local_app_data: Option<PathBuf>) -> Result<PathBuf> {
    settings_base_for(program, local_app_data, msix_family())
}
fn settings_base_for(
    program: &Path,
    local_app_data: Option<PathBuf>,
    family: Option<&str>,
) -> Result<PathBuf> {
    if family.is_none() && (!cfg!(windows) || !is_msi_at(program)) {
        return Ok(program.to_path_buf());
    }
    let local =
        local_app_data.context("Windows did not provide a local application data folder")?;
    ensure!(
        local.is_absolute(),
        "The local application data folder must be absolute"
    );
    if let Some(family) = family {
        ensure!(
            !family.is_empty()
                && family.len() <= 255
                && family
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_')),
            "Invalid MSIX package identity"
        );
        return Ok(local.join("Packages").join(family).join("LocalState"));
    }
    Ok(local.join("LibreTranslate GUI"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn msix_uses_package_data_instead_of_the_protected_program_folder() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("WindowsApps/App");
        let local = dir.path().join("Local");
        assert_eq!(
            settings_base_for(
                &program,
                Some(local.clone()),
                Some("shadersmods.LibreTranslateGUI.Unsigned_123")
            )
            .unwrap(),
            local.join("Packages/shadersmods.LibreTranslateGUI.Unsigned_123/LocalState")
        );
        assert!(settings_base_for(&program, Some(local), Some("../escape")).is_err());
    }

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
