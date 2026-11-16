use crate::{
    components::{Button, Choice, Dropdown, Icon, Input, Switch},
    i18n::Locale,
    manager::Action,
    model::Instance,
    releases, runtime,
    state::AppState,
};
use dioxus::prelude::*;

#[component]
fn RuntimeVersion(instance: Instance, editable: bool) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let s = state.read();
    let locale = s.locale;
    let id = instance.id;
    let selected = instance.server.libretranslate_version.clone();
    let installed = s
        .active_paths
        .as_ref()
        .and_then(|paths| runtime::installed_version(&runtime::instance_dir(&paths.root, id)));
    let catalog = s
        .releases
        .clone()
        .unwrap_or_else(releases::ReleaseCatalog::fallback);
    let mut versions = catalog.versions.clone();
    if !versions.contains(&selected) {
        versions.insert(0, selected.clone());
    }
    let choices = versions
        .into_iter()
        .map(|version| Choice {
            icon: Some("server".into()),
            label: version.clone(),
            value: version,
            disabled: false,
            details: None,
        })
        .collect::<Vec<_>>();
    let outdated = installed
        .as_deref()
        .is_some_and(|version| releases::newer(&catalog.latest, version));
    let needs_install = installed.as_deref() != Some(selected.as_str());
    let latest = catalog.latest.clone();
    let any_install = s.views.iter().any(|view| {
        matches!(
            view.status,
            crate::manager::Status::Installing | crate::manager::Status::UpdatingModels
        )
    });
    let cuda_missing = instance.device == crate::model::Device::Cuda
        && !s.gpu.as_ref().is_some_and(crate::gpu::GpuInfo::available);
    let apply_disabled = !editable
        || s.pending
        || s.has_unsaved(id)
        || !needs_install
        || any_install
        || cuda_missing;
    rsx! {section {class:"advanced-section runtime-version-section",h2 {{locale.text("runtime_version")}}
        div {class:"runtime-version-summary",
            span {{locale.text("runtime_installed_version")} ": " strong {{installed.as_deref().unwrap_or(locale.text("not_installed"))}}}
            span {{locale.text(if catalog.offline {"latest_known_version"} else {"latest_version"})} ": " strong {"{catalog.latest}"}}
        }
        if outdated {p {class:"runtime-update-note",Icon {kind:"download"} {locale.text("runtime_update_available").replace("{version}",&catalog.latest)}}}
        else if installed.is_some()&&!catalog.offline {p {class:"field-hint",{locale.text("runtime_up_to_date")}}}
        div {class:"runtime-version-controls",
            div {class:"field",label {r#for:"runtime-version",{locale.text("selected_version")}} Dropdown {id:"runtime-version",label:locale.text("selected_version"),disabled:!editable,value:selected,choices,onchange:move|value|state.write().edit(id,|draft|draft.server.libretranslate_version=value)}}
            Button {class:"secondary",disabled:!editable||instance.server.libretranslate_version==catalog.latest,onclick:move|_|state.write().edit(id,|draft|draft.server.libretranslate_version=latest.clone()),{locale.text("use_latest_version")}}
            Button {class:"primary",disabled:apply_disabled,onclick:move|_|state.write().send(Action::Install(id)),Icon {kind:"download"}{locale.text("runtime_apply_version")}}
        }
        p {class:"field-hint",{locale.text("runtime_version_hint")}}
        if needs_install&&installed.is_some() {p {class:"field-hint",{locale.text("runtime_not_selected")}}}
        div {class:"runtime-check-row",Button {class:"text-button",disabled:s.release_checking,onclick:move|_|state.write().release_refresh+=1,Icon {kind:"repair"}{locale.text(if s.release_checking {"checking_updates"}else{"check_updates"})}}
            if catalog.offline&&!s.release_checking {span {class:"field-hint",{locale.text("runtime_check_offline")}}}
        }
    } }
}

#[component]
pub fn NumberSetting(
    id: String,
    label_key: &'static str,
    value: Option<u32>,
    onchange: EventHandler<Option<u32>>,
    #[props(default = 1)] min: u32,
    #[props(default)] disabled: bool,
) -> Element {
    let locale = use_context::<Signal<AppState>>().read().locale;
    rsx! { div { class: "field", label { r#for: id.clone(), {locale.text(label_key)} }
        Input { id: id.clone(), disabled, r#type: "number", min: "{min}", max: "4294967295", placeholder: locale.text("unlimited"), value: value.map(|n| n.to_string()).unwrap_or_default(), oninput: move |event:FormEvent| { let value=event.value(); if value.is_empty() { onchange.call(None); } else if let Ok(value)=value.parse() { onchange.call(Some(value)); } } }
    } }
}
#[component]
fn SettingToggle(
    id: String,
    label_key: &'static str,
    checked: bool,
    onchange: EventHandler<bool>,
) -> Element {
    let locale = use_context::<Signal<AppState>>().read().locale;
    rsx! { div { class: "switch-row", label { r#for: id.clone(), {locale.text(label_key)} } Switch { id: id.clone(), class: "dx-switch", checked: Some(checked), on_checked_change: move |value| onchange.call(value), span { class: "switch-thumb" } } } }
}
fn languages(instance: &Instance, locale: Locale) -> Vec<Choice> {
    instance
        .languages
        .split(',')
        .map(|code| {
            Choice::language(
                code,
                format!("{} · {}", locale.text(&format!("language_{code}")), code),
            )
        })
        .collect()
}
#[component]
pub fn AdvancedForm(instance: Instance, editable: bool) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let s = state.read();
    let locale = s.locale;
    let id = instance.id;
    let config = instance.server.clone();
    let mut source = vec![Choice::language("auto", locale.text("detect_language"))];
    source.extend(languages(&instance, locale));
    let mut target = vec![Choice::language("locale", locale.text("browser_language"))];
    target.extend(languages(&instance, locale));
    rsx! { form { class: "advanced-form", onsubmit: move |event| event.prevent_default(),
        fieldset { disabled: !editable,
            RuntimeVersion {instance:instance.clone(),editable}
            section { class: "advanced-section", h2 { {locale.text("network_access")} }
                div { class: "form-grid",
                    div { class: "field", label { r#for: "server-access", {locale.text("listen_on")} }
                        Dropdown { id: "server-access", label: locale.text("listen_on"), disabled: !editable, value: (if config.lan_access { "lan" } else { "local" }).to_string(), choices: vec![Choice::new("local",locale.text("this_device")),Choice::new("lan",locale.text("local_network"))], onchange: move |value:String| state.write().edit(id,|draft|draft.server.lan_access=value=="lan") }
                        if config.lan_access { p { class: "field-hint", {locale.text("lan_access_hint")} } }
                    }
                    div { class: "field", label { r#for: "url-prefix", {locale.text("url_prefix")} } Input { id:"url-prefix", placeholder:"/", value:config.url_prefix.clone(), oninput: move |event:FormEvent| state.write().edit(id,|draft|draft.server.url_prefix=event.value()) } }
                    NumberSetting { id:"request-limit", label_key:"request_limit", min:0, value:config.request_limit, onchange: move |value| state.write().edit(id,|draft|{draft.server.request_limit=value; if value==Some(0) {draft.server.api_keys=true;draft.server.require_api_key=true;} }) }
                    NumberSetting { id:"request-time-cost", label_key:"request_time_cost", value:config.request_time_cost, onchange: move |value| state.write().edit(id,|draft|draft.server.request_time_cost=value) }
                }
            }
            section { class: "advanced-section", h2 { {locale.text("translator_page")} }
                SettingToggle { id:"web-ui", label_key:"web_ui", checked:config.web_ui, onchange: move |value|state.write().edit(id,|draft|draft.server.web_ui=value) }
                div { class: "form-grid",
                    div { class: "field field-wide", label { r#for:"frontend-title", {locale.text("frontend_title")} } Input { id:"frontend-title", maxlength:"128", value:config.frontend_title.clone(), placeholder:"LibreTranslate", oninput:move|event:FormEvent|state.write().edit(id,|draft|draft.server.frontend_title=event.value()) } }
                    div { class: "field", label { r#for:"frontend-source", {locale.text("frontend_source")} } Dropdown { id:"frontend-source", label:locale.text("frontend_source"), disabled:!editable, value:config.frontend_source.clone(), choices:source, onchange:move|value|state.write().edit(id,|draft|draft.server.frontend_source=value) } }
                    div { class: "field", label { r#for:"frontend-target", {locale.text("frontend_target")} } Dropdown { id:"frontend-target", label:locale.text("frontend_target"), disabled:!editable, value:config.frontend_target.clone(), choices:target, onchange:move|value|state.write().edit(id,|draft|draft.server.frontend_target=value) } }
                    NumberSetting { id:"frontend-timeout", label_key:"frontend_timeout", value:Some(config.frontend_timeout), onchange:move|value:Option<u32>|state.write().edit(id,|draft|draft.server.frontend_timeout=value.unwrap_or(0)) }
                    NumberSetting { id:"alternatives-limit", label_key:"alternatives_limit", value:config.alternatives_limit, min:0, onchange:move|value|state.write().edit(id,|draft|draft.server.alternatives_limit=value) }
                }
                SettingToggle { id:"file-translation", label_key:"file_translation", checked:config.file_translation, onchange:move|value|state.write().edit(id,|draft|draft.server.file_translation=value) }
                SettingToggle { id:"suggestions", label_key:"suggestions", checked:config.suggestions, onchange:move|value|state.write().edit(id,|draft|draft.server.suggestions=value) }
            }
            section { class: "advanced-section", h2 { {locale.text("api_access")} }
                SettingToggle { id:"api-keys", label_key:"api_keys", checked:config.api_keys, onchange:move|value|state.write().edit(id,|draft|{draft.server.api_keys=value; if !value { draft.server.require_api_key=false; if draft.server.request_limit==Some(0){draft.server.request_limit=None;} }}) }
                SettingToggle { id:"require-api-key", label_key:"require_api_key", checked:config.require_api_key, onchange:move|value|state.write().edit(id,|draft|{draft.server.require_api_key=value; if value {draft.server.api_keys=true;} else if draft.server.request_limit==Some(0) {draft.server.request_limit=None;} }) }
                if config.api_keys || config.require_api_key || config.request_limit==Some(0) {
                    div { class:"field", label { r#for:"api-key", {locale.text("api_key")} }
                        div { class:"secret-input-row", Input { id:"api-key", r#type:"password", autocomplete:"off", value:config.api_key.clone(), oninput:move|event:FormEvent|state.write().edit(id,|draft|draft.server.api_key=event.value()) }
                            Button { r#type:"button", class:"secondary", onclick:move|_|state.write().edit(id,|draft|draft.server.api_key=uuid::Uuid::new_v4().to_string()), Icon { kind:"repair" } {locale.text("generate_key")} }
                            Button { r#type:"button", class:"secondary square-button", disabled:config.api_key.is_empty(), aria_label:locale.text("copy_key"), title:locale.text("copy_key"), onclick:move|_|state.write().copy(config.api_key.clone()), Icon { kind:"copy" } }
                        }
                    }
                }
                SettingToggle { id:"translation-cache", label_key:"translation_cache", checked:config.translation_cache, onchange:move|value|state.write().edit(id,|draft|draft.server.translation_cache=value) }
            }
            section { class: "advanced-section", h2 { {locale.text("models_and_metrics")} }
                SettingToggle { id:"update-models-start", label_key:"update_models_on_start", checked:config.update_models_on_start, onchange:move|value|state.write().edit(id,|draft|draft.server.update_models_on_start=value) }
                SettingToggle { id:"metrics", label_key:"metrics", checked:config.metrics, onchange:move|value|state.write().edit(id,|draft|draft.server.metrics=value) }
                if config.metrics { div { class:"field", label { r#for:"metrics-token", {locale.text("metrics_token")} } Input { id:"metrics-token", r#type:"password", autocomplete:"off", value:config.metrics_token, oninput:move|event:FormEvent|state.write().edit(id,|draft|draft.server.metrics_token=event.value()) } } }
            }
        }
        if !editable { p { class:"field-hint", {locale.text("stop_to_edit")} } }
        if let Some(key)=s.form_error(&instance) { p { class:"field-error", role:"status", {locale.text(key).replace("{max}", &crate::model::max_threads().to_string())} } }
        else if let Some(status)=s.autosave_status(id) { p { class:"autosave-status", role:"status", {locale.text(status)} } }
    } }
}
