use crate::{
    components::{Button, Choice, Dropdown, Icon, TabContent, TabList, TabTrigger, Tabs},
    i18n::Locale,
    manager::Status,
    model::Instance,
    state::AppState,
};
use dioxus::prelude::*;
use dioxus_core::spawn_forever;
use serde::Deserialize;
use std::{
    io::Read,
    time::{Duration, Instant},
};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Debug)]
pub struct Draft {
    pub input: String,
    pub output: String,
    pub source: String,
    pub target: String,
    pub detected: Option<String>,
    pub revision: u64,
    pub translated_revision: Option<u64>,
    pub pending_revision: Option<u64>,
    pub attempted_revision: Option<u64>,
    pub error: Option<ApiError>,
    changed_at: Instant,
}
impl Default for Draft {
    fn default() -> Self {
        Self {
            input: String::new(),
            output: String::new(),
            source: "auto".into(),
            target: String::new(),
            detected: None,
            revision: 0,
            translated_revision: None,
            pending_revision: None,
            attempted_revision: None,
            error: None,
            changed_at: Instant::now(),
        }
    }
}
impl Draft {
    fn awaiting_result(&self) -> bool {
        !self.input.trim().is_empty()
            && !self.target.is_empty()
            && self.error.is_none()
            && self.translated_revision != Some(self.revision)
    }
    pub fn changed(&mut self) {
        self.output.clear();
        self.detected = None;
        self.revision = self.revision.wrapping_add(1);
        self.translated_revision = None;
        self.attempted_revision = None;
        self.error = None;
        self.changed_at = Instant::now();
    }
    fn ready(&self, limit: Option<u32>) -> bool {
        self.pending_revision.is_none()
            && self.attempted_revision != Some(self.revision)
            && self.translated_revision != Some(self.revision)
            && self.changed_at.elapsed() >= Duration::from_millis(600)
            && !self.input.trim().is_empty()
            && !self.target.is_empty()
            && limit.is_none_or(|limit| self.input.chars().count() <= limit as usize)
    }
    pub fn server_restarted(&mut self) {
        if self.pending_revision.is_none() && self.translated_revision != Some(self.revision) {
            self.attempted_revision = None;
            self.error = None;
        }
    }
    fn finish(&mut self, revision: u64, current: bool, result: Result<Translation, ApiError>) {
        if self.pending_revision != Some(revision) {
            return;
        }
        self.pending_revision = None;
        if !current {
            self.attempted_revision = None;
            return;
        }
        if self.revision != revision {
            return;
        }
        match result {
            Ok(result) => {
                self.output = result.text;
                self.detected = result.detected.map(|detected| detected.language);
                self.translated_revision = Some(revision);
            }
            Err(error) => self.error = Some(error),
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
pub struct Language {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub targets: Vec<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiError {
    Connection,
    Authorization,
    Timeout,
    Invalid,
    FileStorage,
    RateLimited,
    TooLarge,
    Server,
    Cancelled,
}
impl ApiError {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Connection => "translator_unavailable",
            Self::Authorization => "translator_auth_error",
            Self::Timeout => "translator_timeout",
            Self::Invalid => "translator_failed",
            Self::FileStorage => "translator_file_storage_error",
            Self::RateLimited => "translator_rate_limited",
            Self::TooLarge => "translator_too_large",
            Self::Server => "translator_server_error",
            Self::Cancelled => "translator_cancelled",
        }
    }
}
pub(crate) fn status_error(status: reqwest::StatusCode) -> Option<ApiError> {
    match status.as_u16() {
        200..=299 => None,
        401 | 403 => Some(ApiError::Authorization),
        413 => Some(ApiError::TooLarge),
        429 => Some(ApiError::RateLimited),
        500..=599 => Some(ApiError::Server),
        _ => Some(ApiError::Invalid),
    }
}
pub(crate) fn client() -> Result<reqwest::blocking::Client, ApiError> {
    static CLIENT: std::sync::OnceLock<Result<reqwest::blocking::Client, ApiError>> =
        std::sync::OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::blocking::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(120))
                .build()
                .map_err(|_| ApiError::Connection)
        })
        .clone()
}
pub(crate) fn body_error(error: std::io::Error) -> ApiError {
    if error.kind() == std::io::ErrorKind::TimedOut
        || error
            .get_ref()
            .and_then(|cause| cause.downcast_ref::<reqwest::Error>())
            .is_some_and(reqwest::Error::is_timeout)
    {
        ApiError::Timeout
    } else {
        ApiError::Connection
    }
}
pub(crate) fn response<T: serde::de::DeserializeOwned>(
    response: Result<reqwest::blocking::Response, reqwest::Error>,
) -> Result<T, ApiError> {
    let response = response.map_err(|error| {
        if error.is_timeout() {
            ApiError::Timeout
        } else {
            ApiError::Connection
        }
    })?;
    if let Some(error) = status_error(response.status()) {
        return Err(error);
    }
    let mut body = Vec::new();
    response
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut body)
        .map_err(body_error)?;
    if body.len() > 8 * 1024 * 1024 {
        return Err(ApiError::Invalid);
    }
    serde_json::from_slice(&body).map_err(|_| ApiError::Invalid)
}
pub fn languages(instance: &Instance) -> Result<Vec<Language>, ApiError> {
    let data: Vec<Language> = response(
        client()?
            .get(format!(
                "{}/languages",
                instance.url().trim_end_matches('/')
            ))
            .timeout(Duration::from_secs(8))
            .send(),
    )?;
    if data.is_empty()
        || data.iter().any(|language| {
            language.code.is_empty()
                || language.code.len() > 16
                || !language
                    .code
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return Err(ApiError::Invalid);
    }
    Ok(data)
}
#[derive(Clone, Debug, Deserialize)]
pub struct Translation {
    #[serde(rename = "translatedText")]
    pub text: String,
    #[serde(rename = "detectedLanguage")]
    pub detected: Option<Detected>,
}
#[derive(Clone, Debug, Deserialize)]
pub struct Detected {
    pub language: String,
}
pub fn translate(instance: &Instance, draft: &Draft) -> Result<Translation, ApiError> {
    response(client()?.post(format!("{}/translate",instance.url().trim_end_matches('/'))).json(&serde_json::json!({
        "q":draft.input,"source":draft.source,"target":draft.target,"format":"text","api_key":instance.server.api_key
    })).send())
}
fn name(locale: Locale, language: &Language) -> String {
    let text = locale.text(&format!(
        "language_{}",
        crate::i18n::model_code(&language.code)
    ));
    if text == "[missing translation]" {
        language.name.clone()
    } else {
        text.into()
    }
}
fn choices(locale: Locale, languages: &[Language]) -> Vec<Choice> {
    let mut choices = languages
        .iter()
        .map(|language| Choice::language(&language.code, name(locale, language)))
        .collect::<Vec<_>>();
    choices.sort_by(|a, b| a.label.cmp(&b.label));
    choices
}

#[component]
pub fn TranslatorPanel(instance: ReadSignal<Instance>, status: ReadSignal<Status>) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let locale = state.read().locale;
    let id = instance().id;
    use_hook(move || {
        state.write().translations.entry(id).or_default();
    });
    let mut mode = use_signal(|| "text".to_string());
    let mut composing = use_signal(|| false);
    let mut languages = use_resource(move || {
        let instance = instance();
        let running = status() == Status::Running;
        async move {
            if !running {
                return Err(ApiError::Connection);
            }
            tokio::task::spawn_blocking(move || self::languages(&instance))
                .await
                .unwrap_or(Err(ApiError::Connection))
        }
    });
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            if *status.peek() != Status::Running {
                continue;
            }
            if mode.peek().as_str() != "text" || *composing.peek() {
                continue;
            }
            // Inspect cheap request metadata before cloning either text buffer.
            let draft = {
                let state = state.peek();
                let Some(draft) = state.translations.get(&id) else {
                    continue;
                };
                if !draft.ready(instance.peek().server.char_limit) {
                    continue;
                }
                draft.clone()
            };
            let valid = languages
                .peek()
                .as_ref()
                .and_then(|result| result.as_ref().ok())
                .is_some_and(|languages| {
                    target_languages(languages, &draft.source)
                        .iter()
                        .any(|target| target.code == draft.target)
                });
            if !valid {
                continue;
            }
            let revision = draft.revision;
            let instance = instance.peek().clone();
            let pid = state
                .peek()
                .views
                .iter()
                .find(|view| view.instance.id == id)
                .and_then(|view| view.pid);
            {
                let mut state = state.write();
                let draft = state.translations.get_mut(&id).unwrap();
                draft.pending_revision = Some(revision);
                draft.attempted_revision = Some(revision);
                draft.error = None;
            }
            // Only the root-owned state crosses the await. Tab/instance navigation
            // must not cancel delivery or start a second copy of this request.
            spawn_forever(async move {
                let request = instance.clone();
                let result = tokio::task::spawn_blocking(move || translate(&request, &draft))
                    .await
                    .unwrap_or(Err(ApiError::Connection));
                let mut state = state.write();
                let current = state.views.iter().any(|view| {
                    view.instance == instance && view.status == Status::Running && view.pid == pid
                });
                if let Some(draft) = state.translations.get_mut(&id) {
                    draft.finish(revision, current, result);
                }
            });
        }
    });
    use_effect(move || {
        if let Some(Ok(languages)) = &*languages.read() {
            let draft = state
                .read()
                .translations
                .get(&id)
                .cloned()
                .unwrap_or_default();
            let targets = target_languages(languages, &draft.source);
            if !targets.is_empty() && !targets.iter().any(|language| language.code == draft.target)
            {
                let instance = instance();
                let preferred = if instance.server.frontend_target == "locale" {
                    locale.translation_code()
                } else {
                    &instance.server.frontend_target
                };
                let target = targets
                    .iter()
                    .find(|language| {
                        crate::i18n::model_code(&language.code)
                            == crate::i18n::model_code(preferred)
                    })
                    .unwrap_or(&targets[0])
                    .code
                    .clone();
                let mut state = state.write();
                let draft = state.translations.entry(id).or_default();
                draft.target = target;
                draft.changed();
            }
        }
    });
    let draft = state
        .read()
        .translations
        .get(&id)
        .cloned()
        .unwrap_or_default();
    let mut available = languages
        .read()
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .cloned()
        .unwrap_or_default();
    let interactive = status() == Status::Running && !available.is_empty();
    if available.is_empty() {
        available = instance()
            .languages
            .split(',')
            .map(|code| Language {
                name: locale.text(&format!("language_{code}")).into(),
                code: code.to_string(),
                targets: Vec::new(),
            })
            .collect();
    }
    let mut source_choices = choices(locale, &available);
    source_choices.insert(
        0,
        Choice {
            icon: Some("globe".into()),
            value: "auto".into(),
            label: locale.text("detect_language").into(),
            disabled: false,
        },
    );
    let targets = target_languages(&available, &draft.source);
    let target_choices = choices(locale, &targets);
    let count = draft.input.chars().count();
    let limit = instance().server.char_limit;
    let too_long = limit.is_some_and(|limit| count > limit as usize);
    let pending = draft.pending_revision.is_some();
    let loading = interactive && !too_long && draft.awaiting_result();
    let counters = state.read().preferences.translator_counters.clone();
    let words = if counters.words {
        draft.input.unicode_words().count()
    } else {
        0
    };
    let can_swap = interactive && (draft.source != "auto" || draft.detected.is_some());
    let copied = draft.output.clone();
    let detected = draft
        .detected
        .as_ref()
        .and_then(|code| available.iter().find(|language| &language.code == code))
        .map(|language| name(locale, language));
    rsx! {section {class:"translator-panel",
        if status()!=Status::Running {p {class:"translator-offline",role:"status",Icon{kind:"server"}{locale.text(if status()==Status::NotInstalled{"translator_install_first"}else{"translator_start_server"})}}}
        else if !interactive {p {class:"field-hint",{locale.text(if languages.pending(){"translator_loading"}else{"translator_unavailable"})}} Button{class:"secondary",onclick:move |_|languages.restart(),{locale.text("rescan_gpu")}}}
            Tabs{class:"translator-mode",horizontal:true,value:Some(mode()),on_value_change:move|value|mode.set(value),
                TabList{class:"translator-mode-list",
                    TabTrigger{class:"translator-mode-trigger",index:0usize,value:"text",id:"translator-mode-text-tab",Icon{kind:"text"}{locale.text("translator_text")}}
                    TabTrigger{class:"translator-mode-trigger",index:1usize,value:"files",id:"translator-mode-files-tab",Icon{kind:"file"}{locale.text("translator_files")}}
                }
            fieldset{class:"translator-controls",disabled:!interactive,
            div {class:"translator-selectors",
                div {class:"field",label{r#for:"translate-source",{locale.text("translator_from")}} Dropdown {id:"translate-source",label:locale.text("translator_from"),value:draft.source.clone(),choices:source_choices,disabled:!interactive,
                    onchange:move |value|{let mut s=state.write();let draft=s.translations.entry(id).or_default();draft.source=value;draft.changed();}}
                }
                Button{class:"secondary translator-swap",aria_label:locale.text("translator_swap"),title:locale.text("translator_swap"),disabled:!can_swap,onclick:move |_|{
                    let mut s=state.write();let draft=s.translations.entry(id).or_default();let source=if draft.source=="auto"{draft.detected.clone().unwrap_or_default()}else{draft.source.clone()};
                    if !source.is_empty(){draft.source=std::mem::replace(&mut draft.target,source);draft.changed();}
                },Icon{kind:"swap"}}
                div {class:"field",label{r#for:"translate-target",{locale.text("translator_to")}} Dropdown {id:"translate-target",label:locale.text("translator_to"),value:draft.target.clone(),choices:target_choices,disabled:!interactive,
                    onchange:move |value|{let mut s=state.write();let draft=s.translations.entry(id).or_default();draft.target=value;draft.changed();}}
                }
            }
            TabContent{index:0usize,value:"text",aria_labelledby:"translator-mode-text-tab",tabindex:"-1",class:"translator-mode-panel",div {class:"translator-panes",
                div {class:"translator-pane",
                    textarea {id:"translator-input",aria_label:locale.text("translator_input"),placeholder:locale.text("translator_input"),dir:"auto",value:draft.input.clone(),oncompositionstart:move |_|composing.set(true),oncompositionend:move |_|{composing.set(false);state.write().translations.entry(id).or_default().changed();},oninput:move |event|{
                        let mut s=state.write();let draft=s.translations.entry(id).or_default();draft.input=event.value();draft.changed();
                    }}
                    div {class:"translator-pane-footer",div {class:"translator-counters",
                        if counters.characters {span {class:"translator-count",aria_label:format!("{count} {}",locale.text("translator_characters")),title:locale.text("translator_characters"),bdi {dir:"ltr","{count}" if let Some(limit)=limit {" / {limit}"}} if counters.labels {" " {locale.text("translator_characters")}}}}
                        if counters.characters&&counters.words {span {class:"counter-separator",aria_hidden:"true","·"}}
                        if counters.words {span {class:"translator-count",aria_label:format!("{words} {}",locale.text("translator_words")),title:locale.text("translator_words"),bdi {dir:"ltr","{words}"} if counters.labels {" " {locale.text("translator_words")}}}}
                    }
                        Button{class:"text-button",aria_label:locale.text("translator_clear"),title:locale.text("translator_clear"),disabled:count==0,onclick:move |_|{let mut s=state.write();let draft=s.translations.entry(id).or_default();draft.input.clear();draft.changed();},Icon{kind:"close"}}
                    }
                }
                div {class:"translator-pane translator-result",aria_busy:loading,
                    div {class:"translator-result-body",
                        textarea {aria_label:locale.text("translator_output"),placeholder:if loading {""} else {locale.text("translator_output")},dir:"auto",readonly:true,value:draft.output.clone()}
                        if loading {TranslationSkeleton {text:draft.input.clone()}}
                    }
                    div {class:"translator-pane-footer",span{class:"field-hint",if let Some(detected)=detected {{locale.text("translator_detected").replace("{language}",&detected)}}}
                        Button{class:"text-button",aria_label:locale.text("translator_copy"),title:locale.text("translator_copy"),disabled:copied.is_empty(),onclick:move |_|state.write().copy(copied.clone()),Icon{kind:"copy"}}
                    }
                }
            }
            div {class:"translator-actions",
                div {role:"status",aria_live:"polite",
                    if too_long {p{class:"field-error",{locale.text("translator_limit").replace("{max}",&limit.unwrap_or_default().to_string())}}}
                    else if let Some(problem)=draft.error {p{class:"field-error",{locale.text(problem.key())}} Button{class:"secondary tool-button",disabled:pending,aria_label:locale.text("rescan_gpu"),title:locale.text("rescan_gpu"),onclick:move |_|{state.write().translations.entry(id).or_default().changed();},Icon{kind:"repair"}}}
                }
                p{class:"field-hint translator-auto",role:"status",if loading{span{class:"spinner",aria_hidden:"true"}}{locale.text(if loading{"translator_working"}else{"translator_auto"})}}
            }}
            TabContent{index:1usize,value:"files",aria_labelledby:"translator-mode-files-tab",tabindex:"-1",class:"translator-mode-panel",crate::translator_files::FileTranslator{instance:instance(),status:status(),source:draft.source,target:draft.target}}
        }
        }
    }}
}

#[component]
fn TranslationSkeleton(text: ReadSignal<String>) -> Element {
    let id = use_hook(|| format!("translation-skeleton-{}", uuid::Uuid::new_v4()));
    let effect_id = id.clone();
    use_effect(move || {
        let _ = text();
        document::eval(&format!(
            "const skeletonId={};\n{}",
            serde_json::to_string(&effect_id).unwrap(),
            include_str!("../assets/translation-skeleton.js")
        ));
    });
    let cleanup_id = id.clone();
    use_drop(move || {
        document::eval(&format!(
            "window.ltguiSkeletons?.get({})?.dispose();",
            serde_json::to_string(&cleanup_id).unwrap()
        ));
    });
    rsx! {div {id,class:"translation-skeleton",aria_hidden:"true"}}
}

fn target_languages(languages: &[Language], source: &str) -> Vec<Language> {
    let origin = languages.iter().find(|language| language.code == source);
    languages
        .iter()
        .filter(|language| {
            language.code != source
                && origin.is_none_or(|origin| {
                    origin.targets.is_empty() || origin.targets.contains(&language.code)
                })
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn result_loading_covers_debounce_and_requests_but_ends_on_error_clear_or_completion() {
        let mut draft = Draft::default();
        assert!(!draft.awaiting_result());
        draft.input = "Hello, world".into();
        assert!(!draft.awaiting_result());
        draft.target = "ro".into();
        draft.changed();
        assert!(draft.awaiting_result());
        draft.pending_revision = Some(draft.revision);
        draft.finish(draft.revision, true, Err(ApiError::Timeout));
        assert!(!draft.awaiting_result());
        draft.changed();
        draft.pending_revision = Some(draft.revision);
        draft.finish(
            draft.revision,
            true,
            Ok(Translation {
                text: "Salut, lume".into(),
                detected: None,
            }),
        );
        assert!(!draft.awaiting_result());
        draft.input = "New text".into();
        draft.changed();
        assert!(draft.awaiting_result());
        draft.input.clear();
        draft.changed();
        assert!(!draft.awaiting_result());
    }
    #[test]
    fn canonical_api_languages_use_model_names_and_flags() {
        for (api, model) in [("pt-BR", "pb"), ("zh-Hans", "zh"), ("zh-Hant", "zt")] {
            let language = Language {
                code: api.into(),
                name: "Fallback".into(),
                targets: vec![],
            };
            assert_eq!(
                name(Locale::Fr, &language),
                Locale::Fr.text(&format!("language_{model}"))
            );
            assert_eq!(crate::i18n::model_code(api), model);
        }
    }
    #[test]
    fn actionable_http_failures_are_distinct() {
        for (code, error) in [
            (401, ApiError::Authorization),
            (413, ApiError::TooLarge),
            (429, ApiError::RateLimited),
            (503, ApiError::Server),
        ] {
            assert_eq!(
                status_error(reqwest::StatusCode::from_u16(code).unwrap()),
                Some(error)
            );
        }
    }
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    #[test]
    fn stalled_json_body_is_reported_as_timeout_after_headers_arrive() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (finish, wait) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut bytes = [0u8; 1024];
            while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                let count = stream.read(&mut bytes).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&bytes[..count]);
            }
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 64\r\nConnection: close\r\n\r\n")
                .unwrap();
            let _ = wait.recv_timeout(Duration::from_secs(3));
        });
        let client = reqwest::blocking::Client::builder()
            .no_proxy()
            .timeout(Duration::from_millis(500))
            .build()
            .unwrap();
        let reply = client
            .get(format!("http://{address}"))
            .send()
            .expect("response headers must arrive before the body timeout");
        assert_eq!(
            response::<serde_json::Value>(Ok(reply)).unwrap_err(),
            ApiError::Timeout
        );
        let _ = finish.send(());
        server.join().unwrap();
    }
    #[test]
    fn pending_request_survives_navigation_and_blocks_a_second_request() {
        let mut draft = Draft {
            input: "Hello".into(),
            target: "ro".into(),
            changed_at: Instant::now() - Duration::from_secs(1),
            ..Default::default()
        };
        assert!(draft.ready(None));
        draft.pending_revision = Some(0);
        draft.attempted_revision = Some(0);
        assert!(!draft.clone().ready(None));
        draft.finish(
            0,
            true,
            Ok(Translation {
                text: "Bună".into(),
                detected: None,
            }),
        );
        assert_eq!(draft.output, "Bună");
        assert!(!draft.ready(None));
    }
    #[test]
    fn edits_and_server_restarts_discard_old_results_without_stalling() {
        let mut draft = Draft {
            input: "Old".into(),
            target: "ro".into(),
            pending_revision: Some(0),
            attempted_revision: Some(0),
            ..Default::default()
        };
        draft.input = "New".into();
        draft.changed();
        assert!(!draft.ready(None));
        draft.finish(
            0,
            true,
            Ok(Translation {
                text: "Stale".into(),
                detected: None,
            }),
        );
        assert!(draft.output.is_empty());
        assert!(draft.pending_revision.is_none());
        draft.changed_at = Instant::now() - Duration::from_secs(1);
        assert!(draft.ready(None));
        draft.pending_revision = Some(1);
        draft.attempted_revision = Some(1);
        draft.finish(1, false, Err(ApiError::Connection));
        assert!(draft.ready(None));
        assert!(draft.error.is_none());
        draft.attempted_revision = Some(draft.revision);
        draft.error = Some(ApiError::Connection);
        assert!(!draft.ready(None));
        draft.server_restarted();
        assert!(draft.ready(None));
    }
    fn serve(status: &str, body: &str) -> (Instance, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut instance = Instance {
            port: listener.local_addr().unwrap().port(),
            ..Default::default()
        };
        instance.server.url_prefix = "/local".into();
        instance.server.api_key = "fixture-key".into();
        let reply = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let worker = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            loop {
                let count = stream.read(&mut chunk).unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..count]);
                if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&request[..end]);
                    let length = header
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|size| size.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            stream.write_all(reply.as_bytes()).unwrap();
            String::from_utf8(request).unwrap()
        });
        (instance, worker)
    }
    #[test]
    fn translation_uses_local_prefix_auth_and_unicode_without_browsing() {
        let (instance, worker) = serve(
            "200 OK",
            r#"{"translatedText":"Bună","detectedLanguage":{"language":"en","confidence":99}}"#,
        );
        let draft = Draft {
            input: "Hello, 世界".into(),
            target: "ro".into(),
            ..Default::default()
        };
        let result = translate(&instance, &draft).unwrap();
        assert_eq!(result.text, "Bună");
        assert_eq!(result.detected.unwrap().language, "en");
        let request = worker.join().unwrap();
        assert!(request.starts_with("POST /local/translate "));
        let body: serde_json::Value =
            serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body["q"], draft.input);
        assert_eq!(body["api_key"], "fixture-key");
        assert_eq!(body["format"], "text");
    }
    #[test]
    fn reads_actual_server_languages_and_distinguishes_auth_errors() {
        let (instance, worker) = serve(
            "200 OK",
            r#"[{"code":"en","name":"English","targets":["ro"]},{"code":"ro","name":"Romanian","targets":["en"]}]"#,
        );
        let options = languages(&instance).unwrap();
        assert_eq!(target_languages(&options, "en")[0].code, "ro");
        assert_eq!(target_languages(&options, "auto").len(), 2);
        assert!(worker.join().unwrap().starts_with("GET /local/languages "));
        let (instance, worker) = serve("403 Forbidden", r#"{"error":"rejected"}"#);
        assert_eq!(
            translate(&instance, &Draft::default()).unwrap_err(),
            ApiError::Authorization
        );
        worker.join().unwrap();
    }
    #[test]
    fn editing_invalidates_the_old_result_without_losing_the_input() {
        let mut draft = Draft {
            input: "edited text".into(),
            output: "stale".into(),
            detected: Some("en".into()),
            ..Default::default()
        };
        draft.changed();
        assert_eq!(draft.input, "edited text");
        assert!(draft.output.is_empty());
        assert!(draft.detected.is_none());
        assert_eq!(draft.revision, 1);
    }
}
