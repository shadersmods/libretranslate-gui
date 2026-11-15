use crate::{
    components::{Button, Icon, LanguageFlag},
    downloads::{DownloadKind, DownloadPhase, ModelDownload},
    i18n::Locale,
    manager::{Action, Status},
    model::Instance,
    models, runtime,
    state::AppState,
};
use dioxus::prelude::*;
use dioxus_primitives::progress::{Progress, ProgressIndicator};
use std::path::PathBuf;
fn language(locale: Locale, code: &str) -> String {
    let label = locale.text(&format!("language_{code}"));
    if label.starts_with("[missing") {
        code.into()
    } else {
        label.into()
    }
}
#[component]
pub fn ModelTransfer(download: ModelDownload) -> Element {
    let locale = use_context::<Signal<AppState>>().read().locale;
    let mut name = language(locale, &download.source);
    if let Some(target) = &download.target {
        name = format!("{} → {}", name, language(locale, target));
    }
    let total = download
        .total
        .map(models::format_bytes)
        .unwrap_or_else(|| locale.text("download_total_unknown").into());
    let sizes = if download.phase == DownloadPhase::Complete {
        models::format_bytes(download.downloaded)
    } else {
        format!("{} / {}", models::format_bytes(download.downloaded), total)
    };
    let phase = download.phase.label();
    let progress = if matches!(
        download.phase,
        DownloadPhase::Queued | DownloadPhase::Validating
    ) {
        None
    } else {
        download.percent()
    };
    let class = match download.phase {
        DownloadPhase::Complete => "complete",
        DownloadPhase::Failed => "failed",
        DownloadPhase::Interrupted => "interrupted",
        _ => "active",
    };
    rsx! { div {class:"model-transfer {class}",
        div {class:"transfer-heading",
            span {class:"transfer-name", LanguageFlag {code:download.source.clone()} "{name}" if download.kind==DownloadKind::Sentence { span {class:"transfer-kind",{locale.text("sentence_model")}} } }
            span {class:"transfer-phase",{locale.text(phase)}}
        }
        Progress {class:"model-transfer-bar", aria_label:format!("{}: {}",name,locale.text(phase)), aria_valuetext:format!("{} · {}",sizes,locale.text(phase)), value:progress, ProgressIndicator {class:"model-transfer-fill"} }
        div {class:"transfer-footer", span {"{sizes}"} if download.phase!=DownloadPhase::Complete {if let Some(percent)=download.percent() {span {"{percent:.0}%"}}} }
    } }
}
#[component]
fn DownloadSize(url: ReadSignal<String>, revision: ReadSignal<u32>) -> Element {
    let locale = use_context::<Signal<AppState>>().read().locale;
    let size = use_resource(move || {
        let url = url();
        let _ = revision();
        async move { models::download_size(url).await }
    });
    rsx! { span { match &*size.read() {Some(Some(bytes))=>models::format_bytes(*bytes),Some(None)=>locale.text("size_unknown").to_string(),None=>locale.text("checking_size").to_string()} } }
}
#[component]
pub fn ModelsPanel(
    instance: ReadSignal<Instance>,
    status: ReadSignal<Status>,
    root: PathBuf,
    cache: PathBuf,
) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let s = state.read();
    let locale = s.locale;
    let current = instance();
    let id = current.id;
    let installed = runtime::installed(&runtime::instance_dir(&root, id));
    let disabled = status().busy()
        || s.pending
        || s.has_unsaved(id)
        || s.views
            .iter()
            .any(|view| matches!(view.status, Status::Installing | Status::UpdatingModels));
    let mut refresh = use_signal(|| 0u32);
    let snapshot = use_resource(move || {
        let current = instance();
        let _ = status();
        let revision = refresh();
        let root = root.clone();
        let cache = cache.clone();
        async move {
            tokio::task::spawn_blocking(move || {
                models::inspect(&root, &cache, current.id, &current.languages, revision > 0)
            })
            .await
            .map_err(|error| error.to_string())
            .and_then(|result| result.map_err(|error| error.to_string()))
        }
    });
    rsx! { section { class:"models-panel",
        div { class:"section-heading", h2 { {locale.text("translation_models")} }
            div { class:"model-actions",
                Button { class:"secondary", disabled:snapshot.pending(), onclick:move|_|{models::clear_sizes();*refresh.write()+=1;}, Icon { kind:"repair" } {locale.text("refresh_models")} }
                Button { class:"primary", disabled:disabled||!installed, onclick:move|_|state.write().send(Action::UpdateModels(id)), Icon { kind:"download" } {locale.text("update_models")} }
                Button { class:"secondary", disabled:disabled||!installed, onclick:move|_|state.write().send(Action::CleanStorage(id)), Icon { kind:"trash" } {locale.text("clean_storage")} }
            }
        }
        if !installed { p { class:"field-hint", {locale.text("models_install_first")} } }
        if status().active() { p { class:"field-hint", {locale.text("models_stop_first")} } }
        match &*snapshot.read() {
            None=>rsx!{p{class:"muted",{locale.text("loading_models")}}},
            Some(Err(error))=>rsx!{p{class:"field-error",{locale.text("models_load_failed")}} details{summary{{locale.text("technical_details")}} pre{"{error}"}}},
            Some(Ok(snapshot))=>rsx!{
                if snapshot.cached { p { class:"field-hint",{locale.text("models_cached_catalog")} } }
                if snapshot.rows.is_empty() { p {class:"muted",{locale.text("models_no_pairs")}} }
                else { div { class:"model-table-scroll", table { class:"model-table",
                    thead { tr { th {scope:"col",{locale.text("model_pair")}} th {scope:"col",{locale.text("installed_version")}} th {scope:"col",{locale.text("available_version")}} th {scope:"col",{locale.text("model_disk_size")}} th {scope:"col",{locale.text("model_download_size")}} } }
                    tbody { for row in snapshot.rows.clone() { tr { key:"{row.from}-{row.to}",
                        td { class:"model-pair", div {class:"model-pair-content", span {class:"model-language",LanguageFlag{code:row.from.clone()} {language(locale,&row.from)}} Icon {kind:"chevron"} span {class:"model-language",LanguageFlag{code:row.to.clone()} {language(locale,&row.to)}} } }
                        td { span {class:"cell-label",{locale.text("installed_version")}} span { {row.installed.as_deref().unwrap_or(locale.text("not_installed"))} } }
                        td { span {class:"cell-label",{locale.text("available_version")}} span {class:if row.update {"model-update"}else{""}, {row.available.as_deref().unwrap_or("—")} } if row.update { span {class:"update-tag",{locale.text("model_update_available")}} } }
                        td { span {class:"cell-label",{locale.text("model_disk_size")}} {row.disk_bytes.map(models::format_bytes).unwrap_or_else(||"—".into())} }
                        td { span {class:"cell-label",{locale.text("model_download_size")}} if let Some(url)=row.download_url {DownloadSize{url,revision:refresh()}}else{{locale.text("size_unknown")}} }
                    }} }
                } } }
                div {class:"model-storage-notes",
                    if let Some(bytes)=snapshot.storage_bytes {p{class:"field-hint",{locale.text("instance_storage").replace("{size}",&models::format_bytes(bytes))}}}
                    p{class:"field-hint",{locale.text("storage_retention_hint")}}
                }
            },
        }
    } }
}
