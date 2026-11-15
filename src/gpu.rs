use crate::runtime;
use std::{path::PathBuf, time::Duration};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GpuInfo {
    pub devices: Vec<String>,
}
impl GpuInfo {
    pub fn available(&self) -> bool {
        !self.devices.is_empty() && supported_platform()
    }
}
pub fn supported_platform() -> bool {
    matches!(std::env::consts::OS, "windows" | "linux")
        && matches!(std::env::consts::ARCH, "x86_64" | "aarch64")
        && !(cfg!(windows) && cfg!(target_arch = "aarch64"))
}
pub fn detect() -> GpuInfo {
    if !supported_platform() {
        return GpuInfo::default();
    }
    let candidates = vec![PathBuf::from("nvidia-smi")];
    #[cfg(windows)]
    let candidates = {
        let mut candidates = candidates;
        if let Some(windows) = std::env::var_os("WINDIR") {
            candidates.push(PathBuf::from(windows).join("System32/nvidia-smi.exe"));
        }
        if let Some(programs) = std::env::var_os("ProgramFiles") {
            candidates
                .push(PathBuf::from(programs).join("NVIDIA Corporation/NVSMI/nvidia-smi.exe"));
        }
        candidates
    };
    for path in candidates {
        let mut cmd = runtime::command(path);
        cmd.args(["--query-gpu=name,driver_version", "--format=csv,noheader"]);
        if let Ok((true, output)) = runtime::capture(&mut cmd, Duration::from_secs(3)) {
            let devices = parse_devices(&output);
            if !devices.is_empty() {
                return GpuInfo { devices };
            }
        }
    }
    GpuInfo::default()
}
fn parse_devices(output: &str) -> Vec<String> {
    output
        .lines()
        .filter_map(|line| {
            let (name, driver) = line.split_once(',')?;
            let (name, driver) = (name.trim(), driver.trim());
            if name.is_empty() || !driver.bytes().next().is_some_and(|b| b.is_ascii_digit()) {
                None
            } else {
                Some(format!("{name} · {driver}"))
            }
        })
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gpu_probe_requires_device_and_driver_not_an_error_message() {
        assert_eq!(
            parse_devices("NVIDIA GeForce RTX 4090, 581.80\r\n").len(),
            1
        );
        assert!(parse_devices("No devices were found").is_empty());
        assert!(parse_devices("NVIDIA-SMI failed, no driver").is_empty());
    }
}
