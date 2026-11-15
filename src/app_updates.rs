use crate::{
    components::{Button, DialogContent, DialogDescription, DialogRoot, DialogTitle, Icon},
    state::AppState,
};
use dioxus::prelude::*;
use dioxus_primitives::progress::{Progress, ProgressIndicator};
use serde::Deserialize;
use std::{io::Read, time::Duration};

pub const REPOSITORY: &str = "https://github.com/shadersmods/libretranslate-gui";
const ENDPOINT: &str =
    "https://api.github.com/repos/shadersmods/libretranslate-gui/releases/latest";

#[derive(Clone, Debug, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub html_url: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<Asset>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub digest: Option<String>,
}
#[derive(Debug, PartialEq, Eq)]
pub enum CheckError {
    Unavailable,
    Invalid,
}
pub fn newer(tag: &str, current: &str) -> bool {
    let Ok(next) = semver::Version::parse(tag.trim_start_matches('v')) else {
        return false;
    };
    let Ok(current) = semver::Version::parse(current) else {
        return false;
    };
    next.pre.is_empty() && next > current
}
impl Release {
    fn validate(&self) -> bool {
        !self.draft
            && !self.prerelease
            && semver::Version::parse(self.tag_name.trim_start_matches('v'))
                .is_ok_and(|version| version.pre.is_empty())
            && self
                .html_url
                .starts_with(&format!("{REPOSITORY}/releases/tag/"))
            && self
                .body
                .as_ref()
                .is_none_or(|body| body.len() <= 256 * 1024)
    }
    pub fn update_asset(&self) -> Option<&Asset> {
        let name = crate::self_update::package_name(self)?;
        self.assets.iter().find(|asset| {
            asset.name == name
                && asset
                    .browser_download_url
                    .starts_with(&format!("{REPOSITORY}/releases/download/"))
        })
    }
}
pub fn check() -> Result<Option<Release>, CheckError> {
    #[cfg(feature = "ui-test")]
    if let Ok(endpoint) = std::env::var("LIBRETRANSLATE_GUI_TEST_RELEASE_URL") {
        let url = reqwest::Url::parse(&endpoint).map_err(|_| CheckError::Invalid)?;
        if url.scheme() != "http" || url.host_str() != Some("127.0.0.1") {
            return Err(CheckError::Invalid);
        }
        return check_at(&endpoint);
    }
    check_at(ENDPOINT)
}
fn check_at(endpoint: &str) -> Result<Option<Release>, CheckError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(concat!("LibreTranslate-GUI/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| CheckError::Unavailable)?;
    let response = client
        .get(endpoint)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|_| CheckError::Unavailable)?;
    decode(response)
}
fn decode(response: reqwest::blocking::Response) -> Result<Option<Release>, CheckError> {
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if !response.status().is_success() {
        return Err(CheckError::Unavailable);
    }
    let mut body = Vec::new();
    response
        .take(1024 * 1024 + 1)
        .read_to_end(&mut body)
        .map_err(|_| CheckError::Unavailable)?;
    if body.len() > 1024 * 1024 {
        return Err(CheckError::Invalid);
    }
    let release: Release = serde_json::from_slice(&body).map_err(|_| CheckError::Invalid)?;
    if !release.validate() {
        return Err(CheckError::Invalid);
    }
    Ok(Some(release))
}

#[component]
pub fn UpdateCheck() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let revision = use_memo(move || state.read().app_update_refresh);
    use_resource(move || {
        let revision = revision();
        async move {
            state.write().app_update_checking = true;
            let result = tokio::task::spawn_blocking(check)
                .await
                .unwrap_or(Err(CheckError::Unavailable));
            let mut s = state.write();
            s.app_update_checking = false;
            match result {
                Ok(Some(release)) => {
                    let available = newer(&release.tag_name, env!("CARGO_PKG_VERSION"));
                    s.app_update_open = available;
                    s.app_update_status = Some(if available {
                        "app_update_available"
                    } else {
                        "app_up_to_date"
                    });
                    s.app_update = if available { Some(release) } else { None };
                }
                Ok(None) => {
                    s.app_update_status = Some("app_no_releases");
                    s.app_update = None;
                    s.app_update_open = false;
                }
                Err(_) => s.app_update_status = Some("app_update_unavailable"),
            }
            // Startup failures stay in About; manual checks report the same status there.
            let _ = revision;
        }
    });
    rsx! {UpdateDialog{}}
}

#[component]
fn UpdateDialog() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let mut working = use_signal(|| false);
    let shared = use_hook(|| {
        std::sync::Arc::new(std::sync::Mutex::new(
            crate::self_update::Progress::default(),
        ))
    });
    let progress_shared = shared.clone();
    let mut progress = use_signal(crate::self_update::Progress::default);
    use_future(move || {
        let shared = progress_shared.clone();
        async move {
            loop {
                tokio::time::sleep(Duration::from_millis(150)).await;
                if *working.peek() {
                    progress.set(*shared.lock().unwrap());
                }
            }
        }
    });
    let s = state.read();
    let locale = s.locale;
    let Some(release) = s.app_update.clone() else {
        return rsx! {};
    };
    let open = s.app_update_open;
    let supported = release.update_asset().is_some();
    let candidate = release.clone();
    let notes = release
        .body
        .filter(|body| !body.trim().is_empty())
        .unwrap_or_else(|| locale.text("app_no_changelog").into());
    let phase = progress().phase;
    let bytes = format!(
        "{} / {}",
        crate::models::format_bytes(progress().downloaded),
        crate::models::format_bytes(progress().total)
    );
    let percent = if phase == 0 && progress().total > 0 {
        Some(progress().downloaded as f64 / progress().total as f64 * 100.)
    } else {
        None
    };
    rsx! {DialogRoot{id:"app-update-dialog".to_string(),class:"dialog-backdrop",open:Some(open),on_open_change:move|open|if !working(){state.write().app_update_open=open},
        DialogContent{class:"app-update-dialog",
            div{class:"update-heading",Icon{kind:"download"} DialogTitle{{locale.text("app_update_title")}}}
            DialogDescription{{locale.text("app_update_description").replace("{version}",&release.tag_name)}}
            div{class:"update-versions",span{{locale.text("app_current_version")}}code{{env!("CARGO_PKG_VERSION")}}Icon{kind:"chevron"}code{"{release.tag_name}"}}
            h3{{locale.text("app_changelog")}}
            div{class:"update-changelog",dir:"auto",tabindex:"0",for line in notes.lines(){
                if line.starts_with('#'){h4{dir:"auto","{line.trim_start_matches('#').trim()}"}}
                else{p{dir:"auto","{line}"}}
            }}
            p{class:"field-hint",{locale.text("app_download_hint")}}
            if !supported {p{class:"field-hint",{locale.text("app_update_unsupported")}}}
            if working(){div{class:"update-progress",role:"status",p{{locale.text(match phase{1=>"app_update_verifying",2=>"app_update_restarting",_=>"download_active"})}}
                Progress{class:"model-transfer-bar",value:percent,aria_label:locale.text("app_update_title"),ProgressIndicator{class:"model-transfer-fill"}}
                if phase==0 {p{class:"field-hint",dir:"ltr","{bytes}"}}
            }}
            div{class:"dialog-actions",
                Button{class:"secondary",disabled:working(),onclick:move|_|state.write().app_update_open=false,{locale.text("app_update_later")}}
                Button{class:"primary",disabled:working()||!supported,onclick:move|_|{
                    if working(){return;}
                    let release=candidate.clone();let shared=shared.clone();let initial=crate::self_update::Progress{total:release.update_asset().map(|asset|asset.size).unwrap_or_default(),..Default::default()};*shared.lock().unwrap()=initial;progress.set(initial);working.set(true);
                    let backend=state.read().backend.clone(); let pending=state.read().pending_saves.clone();
                    spawn(async move{let result=tokio::task::spawn_blocking(move||crate::self_update::prepare(&release,shared).and_then(|job|{crate::state::flush_pending(&backend,&pending)?;crate::self_update::launch_helper(&job)})).await;
                        match result{Ok(Ok(()))=>crate::app::finish_close(state),Ok(Err(error))=>{working.set(false);state.write().error("app_update_failed",format!("{error:#}"));},Err(error)=>{working.set(false);state.write().error("app_update_failed",error.to_string());}}
                    });
                },Icon{kind:"download"}{locale.text("app_download_update")}}
            }
        }
    }}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_endpoint_distinguishes_no_release_offline_and_valid_updates() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        for (status, body, expected) in [
            ("404 Not Found", "{}", 0),
            ("429 Too Many Requests", "{}", 1),
            ("200 OK", "not JSON", 1),
            (
                "200 OK",
                r#"{"tag_name":"v0.3.0","html_url":"https://github.com/shadersmods/libretranslate-gui/releases/tag/v0.3.0","body":"Fixes and improvements","assets":[]}"#,
                2,
            ),
            (
                "200 OK",
                r#"{"tag_name":"v0.3.0","html_url":"https://example.com/other"}"#,
                1,
            ),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let endpoint = format!("http://{}", listener.local_addr().unwrap());
            let reply = format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let server = std::thread::spawn(move || {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = [0; 4096];
                let len = stream.read(&mut request).unwrap();
                assert!(String::from_utf8_lossy(&request[..len]).contains("LibreTranslate-GUI/"));
                stream.write_all(reply.as_bytes()).unwrap();
            });
            let result = check_at(&endpoint);
            assert_eq!(
                match result {
                    Ok(None) => 0,
                    Err(_) => 1,
                    Ok(Some(_)) => 2,
                },
                expected
            );
            server.join().unwrap();
        }
    }
    #[test]
    fn versions_are_semantic_and_stable() {
        assert!(newer("v0.10.0", "0.9.9"));
        assert!(!newer("v0.2.7", "0.2.7"));
        assert!(!newer("v0.3.0-beta.1", "0.2.7"));
        assert!(!newer("broken", "0.2.7"));
    }
    #[test]
    fn only_official_release_urls_are_offered() {
        let mut release:Release=serde_json::from_value(serde_json::json!({"tag_name":"v0.2.8","html_url":format!("{REPOSITORY}/releases/tag/v0.2.8"),"assets":[{"name":"app-Windows-x64.zip","browser_download_url":"https://evil.invalid/app.zip"}]})).unwrap();
        assert!(release.validate());
        assert!(release.update_asset().is_none());
        release.html_url = "https://github.com.evil.invalid/releases/tag/v0.2.8".into();
        assert!(!release.validate());
    }
}
