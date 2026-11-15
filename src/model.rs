use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use uuid::Uuid;

pub fn max_threads() -> u16 {
    std::thread::available_parallelism()
        .map(|n| n.get().min(u16::MAX as usize) as u16)
        .unwrap_or(1)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Device {
    #[default]
    Cpu,
    Cuda,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ServerOptions {
    pub libretranslate_version: String,
    pub lan_access: bool,
    pub char_limit: Option<u32>,
    pub batch_limit: Option<u32>,
    pub request_limit: Option<u32>,
    pub request_time_cost: Option<u32>,
    pub alternatives_limit: Option<u32>,
    pub frontend_title: String,
    pub frontend_source: String,
    pub frontend_target: String,
    pub frontend_timeout: u32,
    pub web_ui: bool,
    pub file_translation: bool,
    pub suggestions: bool,
    pub api_keys: bool,
    pub require_api_key: bool,
    pub api_key: String,
    pub translation_cache: bool,
    pub update_models_on_start: bool,
    pub metrics: bool,
    pub metrics_token: String,
    pub url_prefix: String,
}
impl Default for ServerOptions {
    fn default() -> Self {
        Self {
            libretranslate_version: crate::releases::default_version(),
            lan_access: false,
            char_limit: None,
            batch_limit: None,
            request_limit: None,
            request_time_cost: None,
            alternatives_limit: None,
            frontend_title: String::new(),
            frontend_source: "auto".into(),
            frontend_target: "locale".into(),
            frontend_timeout: 500,
            web_ui: true,
            file_translation: true,
            suggestions: false,
            api_keys: false,
            require_api_key: false,
            api_key: String::new(),
            translation_cache: false,
            update_models_on_start: false,
            metrics: false,
            metrics_token: String::new(),
            url_prefix: String::new(),
        }
    }
}
impl ServerOptions {
    pub fn validate(&mut self, languages: &[String]) -> Result<()> {
        if !crate::releases::supported(&self.libretranslate_version) {
            bail!("Advanced settings: choose a supported stable LibreTranslate version.");
        }
        if self.frontend_timeout == 0 {
            bail!("Advanced settings: frontend timeout must be positive.");
        }
        if self.frontend_title.chars().count() > 128
            || self.metrics_token.len() > 512
            || self.api_key.len() > 512
        {
            bail!("Advanced settings: a title or token is too long.");
        }
        if [self.char_limit, self.batch_limit, self.request_time_cost]
            .into_iter()
            .flatten()
            .any(|limit| limit == 0)
        {
            bail!("Advanced settings: limits must be positive or left empty.");
        }
        if self.require_api_key || self.request_limit == Some(0) {
            self.api_keys = true;
        }
        if (self.require_api_key || self.request_limit == Some(0)) && self.api_key.trim().is_empty()
        {
            bail!("Advanced settings: generate an API key before requiring one.");
        }
        self.url_prefix = self.url_prefix.trim().trim_end_matches('/').to_string();
        if !self.url_prefix.is_empty()
            && (!self.url_prefix.starts_with('/')
                || self.url_prefix.len() > 128
                || self.url_prefix.contains("..")
                || !self
                    .url_prefix
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"/-_".contains(&b)))
        {
            bail!("Advanced settings: use a URL prefix such as /translate.");
        }
        if self.frontend_source != "auto" && !languages.contains(&self.frontend_source) {
            self.frontend_source = "auto".into();
        }
        if self.frontend_target != "locale" && !languages.contains(&self.frontend_target) {
            self.frontend_target = "locale".into();
        }
        Ok(())
    }
    pub fn args(&self) -> Vec<String> {
        let mut args = Vec::new();
        for (flag, value) in [
            ("--char-limit", self.char_limit),
            ("--batch-limit", self.batch_limit),
            ("--req-limit", self.request_limit),
            ("--req-time-cost", self.request_time_cost),
        ] {
            if let Some(value) = value {
                args.extend([flag.to_string(), value.to_string()]);
            }
        }
        if let Some(limit) = self.alternatives_limit {
            args.extend(["--alternatives-limit".into(), limit.to_string()]);
        }
        args.extend([
            "--frontend-timeout".into(),
            self.frontend_timeout.to_string(),
            "--frontend-language-source".into(),
            self.frontend_source.clone(),
            "--frontend-language-target".into(),
            self.frontend_target.clone(),
        ]);
        for (flag, enabled) in [
            ("--disable-web-ui", !self.web_ui),
            ("--disable-files-translation", !self.file_translation),
            ("--suggestions", self.suggestions),
            (
                "--api-keys",
                self.api_keys || self.require_api_key || self.request_limit == Some(0),
            ),
            ("--under-attack", self.require_api_key),
            ("--update-models", self.update_models_on_start),
            ("--metrics", self.metrics),
        ] {
            if enabled {
                args.push(flag.into());
            }
        }
        if self.translation_cache {
            args.extend(["--translation-cache".into(), "all".into()]);
        }
        if !self.frontend_title.is_empty() {
            args.extend(["--frontend-title".into(), self.frontend_title.clone()]);
        }
        if !self.url_prefix.is_empty() {
            args.extend(["--url-prefix".into(), self.url_prefix.clone()]);
        }
        // Secrets use environment variables rather than visible command-line arguments.
        args
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Instance {
    pub id: Uuid,
    pub name: String,
    pub port: u16,
    pub languages: String,
    pub threads: u16,
    #[serde(default)]
    pub device: Device,
    #[serde(default)]
    pub server: ServerOptions,
}

impl Default for Instance {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            name: "My translator".into(),
            port: 5000,
            languages: "en,es".into(),
            threads: max_threads().min(4),
            device: Device::Cpu,
            server: ServerOptions::default(),
        }
    }
}

impl Instance {
    pub fn validate(&mut self, others: &[Instance]) -> Result<()> {
        self.name = self.name.trim().to_owned();
        if self.name.is_empty() || self.name.chars().count() > 64 {
            bail!("Enter an instance name between 1 and 64 characters.");
        }
        if self.port < 1024 {
            bail!("Choose a port between 1024 and 65535.");
        }
        if others
            .iter()
            .any(|i| i.id != self.id && i.port == self.port)
        {
            bail!(
                "Port {} is already assigned to another instance.",
                self.port
            );
        }
        if !(1..=max_threads()).contains(&self.threads) {
            bail!("Choose between 1 and {} worker threads.", max_threads());
        }
        let mut languages = Vec::new();
        for code in self.languages.split(',') {
            let code = code.trim().to_ascii_lowercase();
            if !(2..=12).contains(&code.len())
                || !code.bytes().all(|b| b.is_ascii_lowercase() || b == b'-')
            {
                bail!("Use comma-separated language codes, for example en,es,fr.");
            }
            if !languages.contains(&code) {
                languages.push(code);
            }
        }
        if languages.len() < 2 {
            bail!("Choose at least two languages so the server can translate between them.");
        }
        self.server.validate(&languages)?;
        self.languages = languages.join(",");
        Ok(())
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}{}", self.port, self.server.url_prefix)
    }
}

pub fn load(root: &Path) -> Result<Vec<Instance>> {
    let path = root.join("instances.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut instances: Vec<Instance> =
        serde_json::from_slice(&fs::read(&path)?).with_context(|| {
            format!(
                "Could not read {}. Restore a valid configuration before reopening the app.",
                path.display()
            )
        })?;
    // Portable configurations may come from a machine with more logical CPUs.
    for instance in &mut instances {
        instance.threads = instance.threads.min(max_threads());
    }
    for index in 0..instances.len() {
        let mut checked = instances[index].clone();
        checked.validate(&instances)?;
        if instances[..index].iter().any(|i| i.id == checked.id) {
            bail!("The configuration contains duplicate instance IDs.");
        }
        instances[index] = checked;
    }
    Ok(instances)
}

pub fn save(root: &Path, instances: &[Instance]) -> Result<()> {
    fs::create_dir_all(root)?;
    let temp = root.join("instances.json.tmp");
    let mut file = fs::File::create(&temp)?;
    use std::io::Write;
    file.write_all(&serde_json::to_vec_pretty(instances)?)?;
    file.sync_all()?;
    fs::rename(temp, root.join("instances.json"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn advanced_options_preserve_defaults_validate_prefixes_and_hide_secrets_from_arguments() {
        let mut instance = Instance::default();
        let mut json = serde_json::to_value(&instance).unwrap();
        json.as_object_mut().unwrap().remove("server");
        assert_eq!(
            serde_json::from_value::<Instance>(json).unwrap().server,
            ServerOptions::default()
        );
        instance.server.url_prefix = "/translator/".into();
        instance.validate(&[]).unwrap();
        assert!(instance.url().ends_with("/translator"));
        instance.server.api_key = "private-key".into();
        instance.server.metrics_token = "private-token".into();
        instance.server.require_api_key = true;
        instance.server.metrics = true;
        instance.server.translation_cache = true;
        let args = instance.server.args();
        assert!(args.iter().any(|arg| arg == "--under-attack"));
        assert!(args.iter().any(|arg| arg == "--api-keys"));
        assert!(!args.iter().any(|arg| arg.contains("private-")));
        instance.server.url_prefix = "/../../escape".into();
        assert!(instance.validate(&[]).is_err());
    }

    #[test]
    fn threads_follow_the_host_limit_and_portable_configs_are_capped() {
        let maximum = max_threads();
        let mut instance = Instance {
            threads: maximum,
            ..Default::default()
        };
        assert!(instance.validate(&[]).is_ok());
        instance.threads = 0;
        assert!(instance.validate(&[]).is_err());
        if maximum < u16::MAX {
            instance.threads = maximum + 1;
            assert!(instance.validate(&[]).is_err());
            let root = tempfile::tempdir().unwrap();
            save(root.path(), &[instance]).unwrap();
            assert_eq!(load(root.path()).unwrap()[0].threads, maximum);
        }
    }

    #[test]
    fn old_configs_default_to_cpu_and_cuda_roundtrips() {
        let instance = Instance::default();
        let mut value = serde_json::to_value(&instance).unwrap();
        value.as_object_mut().unwrap().remove("device");
        assert_eq!(
            serde_json::from_value::<Instance>(value).unwrap().device,
            Device::Cpu
        );
        let cuda = Instance {
            device: Device::Cuda,
            ..instance
        };
        assert_eq!(
            serde_json::from_slice::<Instance>(&serde_json::to_vec(&cuda).unwrap())
                .unwrap()
                .device,
            Device::Cuda
        );
    }

    #[test]
    fn validates_and_normalizes_languages() {
        let mut instance = Instance {
            languages: " EN, es,en, pt-br ".into(),
            ..Default::default()
        };
        instance.validate(&[]).unwrap();
        assert_eq!(instance.languages, "en,es,pt-br");
        instance.languages = "en,../../escape".into();
        assert!(instance.validate(&[]).is_err());
    }

    #[test]
    fn rejects_port_conflicts_but_allows_editing_self() {
        let mut instance = Instance::default();
        assert!(instance.validate(&[instance.clone()]).is_ok());
        assert!(Instance::default().validate(&[instance]).is_err());
    }

    #[test]
    fn config_roundtrip_and_corruption() {
        let root = tempfile::tempdir().unwrap();
        let instance = Instance::default();
        save(root.path(), std::slice::from_ref(&instance)).unwrap();
        assert_eq!(load(root.path()).unwrap()[0].id, instance.id);
        fs::write(root.path().join("instances.json"), b"invalid").unwrap();
        assert!(load(root.path()).is_err());
    }
}
