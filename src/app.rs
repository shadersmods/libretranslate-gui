use crate::advanced::{AdvancedForm, NumberSetting};
use crate::model_ui::ModelsPanel;
use crate::{
    components::{
        Button, Choice, DialogContent, DialogDescription, DialogRoot, DialogTitle, Dropdown, Icon,
        Input, InstanceIcon, LanguageFlag, Switch, TabContent, TabList, TabTrigger, Tabs,
    },
    gpu,
    i18n::Locale,
    logs,
    manager::{Action, Status},
    model::{Device, Instance},
    network, runtime,
    settings::{Preferences, Theme},
    state::{AppState, InitialState, Page},
};
use dioxus::prelude::*;
use dioxus_primitives::{
    toast::{self, ToastOptions},
    tooltip,
};
use std::{sync::LazyLock, time::Duration};
use uuid::Uuid;

pub const ICON: Asset = asset!("/assets/icon.png");
pub fn icon_uri() -> String {
    ICON.to_string()
}
fn locale_choices() -> Vec<Choice> {
    Locale::ALL
        .into_iter()
        .map(|locale| {
            let mut choice = Choice::language(locale.code(), locale.text("locale_name"));
            choice.icon = Some(locale.translation_code().into());
            choice
        })
        .collect()
}
fn theme_choices(locale: Locale) -> Vec<Choice> {
    ["light", "dark", "system"]
        .into_iter()
        .map(|code| Choice::new(code, locale.text(code)))
        .collect()
}

pub fn app() -> Element {
    let initial = use_context::<InitialState>();
    let mut state = use_signal(|| {
        initial
            .lock()
            .expect("Startup state lock poisoned")
            .take()
            .expect("App initialized twice")
    });
    use_context_provider(|| state);
    let mut maximized = use_signal(|| dioxus::desktop::window().is_maximized());
    use_context_provider(|| crate::components::TooltipController(Signal::new(None)));
    #[cfg(windows)]
    use_future(|| async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        use dioxus::desktop::tao::platform::windows::WindowExtWindows;
        let window = dioxus::desktop::window();
        window.set_undecorated_shadow(false);
        crate::window_chrome::borderless(window.hwnd() as isize);
    });
    let release_revision = use_memo(move || state.read().release_refresh);
    let release_cache = use_hook(move || {
        state
            .peek()
            .active_paths
            .as_ref()
            .map(|paths| paths.cache.clone())
    });
    use_resource(move || {
        let revision = release_revision();
        let cache = release_cache.clone();
        async move {
            state.write().release_checking = true;
            let catalog = if let Some(cache) = cache {
                tokio::task::spawn_blocking(move || crate::releases::check(&cache, revision > 0))
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .unwrap_or_else(crate::releases::ReleaseCatalog::fallback)
            } else {
                crate::releases::ReleaseCatalog::fallback()
            };
            let mut s = state.write();
            s.releases = Some(catalog);
            s.release_checking = false;
        }
    });
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(3600)).await;
            state.write().release_refresh += 1;
        }
    });
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_millis(150)).await;
            let update = state.peek().collect_poll();
            if let Some(update) = update {
                state.write().poll(update);
            }
            if state
                .peek()
                .close_requested
                .swap(false, std::sync::atomic::Ordering::Relaxed)
            {
                request_close(state);
            }
            let current = dioxus::desktop::window().is_maximized();
            if *maximized.peek() != current {
                maximized.set(current);
            }
        }
    });
    use_future(move || async move {
        let info = tokio::task::spawn_blocking(gpu::detect)
            .await
            .unwrap_or_default();
        state.write().gpu = Some(info);
    });
    let locale = state.read().locale;
    let page = state.read().page;
    let theme = state.read().preferences.theme;
    let expanded = state.read().sidebar_expanded;
    let is_maximized = maximized();
    rsx! {
        document::Title { "LibreTranslate GUI" }
        style { {include_str!("../assets/app.css")} }
        div { class: format!("app-shell {} {}", if expanded { "sidebar-expanded" } else { "" }, if is_maximized { "window-maximized" } else { "" }), lang: locale.code(), dir: locale.direction(), "data-theme": theme.code(),
          toast::ToastProvider { class: "toast-region", max_toasts: 4usize, render_toast: |props: toast::ToastPropsWithOwner| rsx! { AppToast { ..props } },
            Titlebar {maximized:is_maximized}
            Rail {}

            div { class: "workspace",
                main { class: "main-content", id: "main-content", inert: state.read().closing.then_some(""),
                    MessageBanner {}
                    if page == Page::Options { Options {} } else { Instances {} }
                }

            }
            RemoveDialog {}
            crate::app_updates::UpdateCheck {}
            if !is_maximized { WindowResize {} }
          }
        }
    }
}

pub fn finish_close(mut state: Signal<AppState>) {
    state.write().closing = true;
    state.read().pending_saves.lock().unwrap().clear();
    let window = dioxus::desktop::window();
    window.set_close_behavior(dioxus::desktop::WindowCloseBehaviour::WindowCloses);
    window.close();
}

pub fn request_close(mut state: Signal<AppState>) {
    if state.peek().closing {
        return;
    }
    state.write().closing = true;
    let backend = state.read().backend.clone();
    let pending = state.read().pending_saves.clone();
    spawn(async move {
        let result =
            tokio::task::spawn_blocking(move || crate::state::flush_pending(&backend, &pending))
                .await;
        match result {
            Ok(Ok(())) => finish_close(state),
            error => {
                let details = match error {
                    Ok(Err(error)) => format!("{error:#}"),
                    Err(error) => error.to_string(),
                    _ => unreachable!(),
                };
                state.write().closing = false;
                state.write().error("close_save_failed", details);
                // Official Dioxus hides the window on a native close request.
                // Bring the unchanged draft and save error back for retry.
                let window = dioxus::desktop::window();
                window.set_visible(true);
                window.set_focus();
            }
        }
    });
}

#[component]
fn Titlebar(maximized: bool) -> Element {
    let state = use_context::<Signal<AppState>>();
    let locale = state.read().locale;
    rsx! { header { class: "custom-titlebar",
        onmousedown: move |event: MouseEvent| { if event.trigger_button() == Some(dioxus::html::input_data::MouseButton::Primary) { dioxus::desktop::window().drag(); } },
        ondoubleclick: move |_| dioxus::desktop::window().toggle_maximized(),
        div { class: "titlebar-brand", img { src: ICON.to_string(), alt: "", width: "26", height: "26" } strong { "LibreTranslate" } span { "GUI" } }
        div { class: "window-controls", onmousedown: move |event| event.stop_propagation(), ondoubleclick: move |event| event.stop_propagation(),
            crate::tasks::TasksDropdown {}
            Button { class: "window-control", aria_label: locale.text("minimize"), onclick: move |_| dioxus::desktop::window().set_minimized(true), Icon { kind: "minimize" } }
            Button { class: "window-control", aria_label: locale.text(if maximized { "restore_window" } else { "maximize" }), onclick: move |_| dioxus::desktop::window().toggle_maximized(), Icon { kind: if maximized { "restore" } else { "maximize" } } }
            Button { class: "window-control window-close", aria_label: locale.text("close_window"), onclick: move |_| request_close(state), Icon { kind: "close" } }
        }
    } }
}

#[component]
fn WindowResize() -> Element {
    use dioxus::desktop::tao::window::ResizeDirection::*;
    rsx! { for (name, direction) in [("n", North), ("s", South), ("e", East), ("w", West), ("ne", NorthEast), ("nw", NorthWest), ("se", SouthEast), ("sw", SouthWest)] {
        div { class: "window-resize {name}", aria_hidden: "true", onmousedown: move |_| { let _ = dioxus::desktop::window().drag_resize_window(direction); } }
    } }
}

#[component]
fn AppToast(props: toast::ToastProps) -> Element {
    let state = use_context::<Signal<AppState>>();
    let s = state.read();
    let locale = s.locale;
    let details = props.description.is_some();
    rsx! { toast::Toast {
        id: props.id, index: props.index, title: props.title, description: props.description, toast_type: props.toast_type, on_close: props.on_close, permanent: props.permanent, duration: props.duration,
        class: "app-toast toast-theme", "data-theme": s.preferences.theme.code(),
        toast::ToastContent { class: "toast-content", toast::ToastTitle { class: "toast-title" }
            if details { details { class: "toast-diagnostics", summary { {locale.text("technical_details")} } toast::ToastDescription { class: "toast-description" } } }
        }
        toast::ToastCloseButton { class: "toast-close", aria_label: locale.text("dismiss"), Icon { kind: "close" } }
    } }
}

#[component]
fn Rail() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let s = state.read();
    let locale = s.locale;
    rsx! { aside { class: "icon-rail",

        Button { class: "rail-button rail-expand", title: locale.text(if s.sidebar_expanded { "collapse_sidebar" } else { "expand_sidebar" }), aria_label: locale.text(if s.sidebar_expanded { "collapse_sidebar" } else { "expand_sidebar" }), aria_expanded: s.sidebar_expanded,
            onclick: move |_| { let expanded = state.read().sidebar_expanded; state.write().sidebar_expanded = !expanded; }, Icon { kind: "chevron" } span { class: "rail-label", {locale.text("collapse_sidebar")} }
        }
        Button { class: if s.page == Page::Instances && s.selected.is_none() { "rail-button active" } else { "rail-button" }, title: locale.text("instances"), aria_label: locale.text("instances"), onclick: move |_| { let mut s = state.write(); s.page = Page::Instances; s.selected = None; s.adding = false; s.editor = None; }, Icon { kind: "server" } span { class: "rail-label", {locale.text("instances")} } }
        div { class: "rail-instance-list",
            for view in s.views.clone() {
                RailInstance {key:"{view.instance.id}",selected:s.page==Page::Instances&&s.selected==Some(view.instance.id),view}
            }
        }
        Button { class: "rail-button rail-add", title: locale.text("add_instance"), aria_label: locale.text("add_instance"), disabled: s.fatal.is_some(), onclick: move |_| state.write().add(), Icon { kind: "plus" } span { class: "rail-label", {locale.text("add_instance")} } }
        div { class: "rail-spacer" }
        if let Some(release)=s.app_update.as_ref() {
            Button {class:"rail-button rail-update",title:format!("{}: {}",locale.text("app_update_available"),release.tag_name),aria_label:format!("{}: {}",locale.text("app_update_available"),release.tag_name),aria_haspopup:"dialog",aria_controls:"app-update-content",aria_expanded:s.app_update_open,onclick:move |_|state.write().app_update_open=true,
                Icon {kind:"download"} span {class:"rail-update-dot",aria_hidden:"true"} span {class:"rail-label",{locale.text("app_update_available")}}
            }
        }
        Button { class: "rail-button", title: locale.text("theme"), aria_label: locale.text("theme"), onclick: move |_| { let theme = state.read().preferences.theme; state.write().set_theme(if theme == Theme::Light { Theme::Dark } else { Theme::Light }); }, Icon { kind: "sun" } span { class: "rail-label", {locale.text("theme")} } }
        Button { class: if s.page == Page::Options { "rail-button active" } else { "rail-button" }, title: locale.text("options"), aria_label: locale.text("options"), onclick: move |_| state.write().page = Page::Options, Icon { kind: "options" } span { class: "rail-label", {locale.text("options")} } }
    } }
}

#[component]
fn RailInstance(view: crate::manager::View, selected: bool) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let s = state.read();
    let locale = s.locale;
    let id = view.instance.id;
    let name = view.instance.name.clone();
    let (type_icon, type_label) = type_identity(view.instance.kind);
    let tooltip_id = use_hook(|| format!("instance-preview-{}", Uuid::new_v4()));
    let anchor_id = tooltip_id.clone();
    let mut active = use_context::<crate::components::TooltipController>().0;
    let change_id = tooltip_id.clone();
    let focus_id = tooltip_id.clone();
    let blur_id = tooltip_id.clone();
    let open = active.read().as_deref() == Some(tooltip_id.as_str());
    rsx! {tooltip::Tooltip {class:"tooltip-root",open:Some(open),on_open_change:move|open|{if open {active.set(Some(change_id.clone()));}else{crate::components::dismiss_tooltip(active,&change_id);}},
        tooltip::TooltipTrigger {class:"tooltip-trigger",tabindex:"-1",
            Button {class:if selected {"rail-instance selected"}else{"rail-instance"},aria_label:name.clone(),aria_describedby:tooltip_id.clone(),"data-instance-preview":tooltip_id.clone(),onclick:move|_|state.write().select(id),
                onfocus:move |_|active.set(Some(focus_id.clone())),
                onblur:move |_|crate::components::dismiss_tooltip(active,&blur_id),
                span {class:"instance-avatar",InstanceIcon {languages:s.avatar_languages(&view.instance)} span {class:"status-dot {status_class(view.status)}"}}
                span {class:"rail-label rail-instance-details",strong {"{name}"} span {{locale.status(view.status)} if !view.instance.is_remote() {" · :{view.instance.port}"}}}
            }
        }
        tooltip::TooltipContent {id:tooltip_id,class:"app-tooltip instance-tooltip",side:dioxus_primitives::ContentSide::Right,
            div {onmounted:move |_|crate::components::position_tooltip(anchor_id.clone(),Some(format!("[data-instance-preview=\"{anchor_id}\"]")),active),
                div {class:"preview-heading",InstanceIcon {languages:s.avatar_languages(&view.instance)} strong {"{name}"}}
                code {class:"preview-endpoint","{view.instance.url()}"}
                InstanceLanguageBadges {languages:s.avatar_languages(&view.instance), max_visible:3}
                div {class:"preview-meta",span {class:"status-badge {status_class(view.status)}",span {class:"status-dot {status_class(view.status)}"} {locale.status(view.status)}}
                    div {class:"card-environment",
                        span {class:"card-type",role:"img",aria_label:locale.text(type_label),title:locale.text(type_label),Icon {kind:type_icon}}
                        if !view.instance.is_remote() {span {class:"card-device",Icon {kind:if view.instance.device==Device::Cuda {"cuda"}else{"cpu"}} {locale.text(if view.instance.device==Device::Cuda {"cuda"}else{"cpu"})}}}
                    }
                }
            }
        }
    } }
}

#[component]
fn MessageBanner() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let notifications = toast::use_toast();
    use_effect(move || {
        let message = state.read().message.clone();
        if let Some(message) = message {
            let locale = state.read().locale;
            state.write().message = None;
            let mut options = ToastOptions::new()
                .duration(Duration::from_secs(5))
                .permanent(message.error);
            if let Some(details) = message.details {
                options = options.description(details);
            }
            if message.error {
                notifications.error(
                    locale
                        .text(message.key)
                        .replace("{max}", &crate::model::max_threads().to_string()),
                    options,
                );
            } else {
                notifications.success(locale.text(message.key).to_string(), options);
            }
        }
    });
    rsx! {}
}
#[component]
fn Instances() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let s = state.read();
    let locale = s.locale;
    if let Some(error) = s.fatal.clone() {
        return rsx! { section { class: "empty-state", h1 { {locale.text("manager_failed")} } p { {locale.text("storage_unavailable")} } details { summary { {locale.text("technical_details")} } pre { "{error}" } } Button { class: "primary", onclick: move |_| state.write().page = Page::Options, {locale.text("options")} } } };
    }
    if s.adding {
        let instance = s.editor.clone().expect("Missing new-instance draft");
        return rsx! { section { class: "add-page", h1 { {locale.text("add_title")} }
            if instance.is_remote() {RemoteForm {instance,creating:true,editable:!s.pending}}else{InstanceForm { instance, creating: true, editable: !s.pending }}
        } };
    }
    let view = s
        .views
        .iter()
        .find(|v| Some(v.instance.id) == s.selected)
        .cloned();
    match view {
        // Keyed iteration remounts instance-local hooks when the selection changes.
        Some(view) => {
            rsx! { for view in [view] { if view.instance.is_remote() { RemoteDetails {key:"{view.instance.id}",id:view.instance.id} }else{InstanceDetails { key:"{view.instance.id}", id: view.instance.id }} } }
        }
        None if !s.views.is_empty() => rsx! { Library {} },
        None => rsx! { section { class: "empty-state",
            img { class: "empty-icon", src: ICON.to_string(), alt: "", width: "88", height: "88" }
            h1 { {locale.text("empty_title")} }
            p { {locale.text("empty_description")} }
            Button { class: "primary", onclick: move |_| state.write().add(), Icon { kind: "plus" } {locale.text("create_instance")} }
            p { class: "field-hint", {locale.text("setup_note")} }
        } },
    }
}

fn type_identity(kind: crate::model::InstanceKind) -> (&'static str, &'static str) {
    match kind {
        crate::model::InstanceKind::Local => ("computer", "local_instance"),
        crate::model::InstanceKind::LibreTranslate => ("cloud", "hosted"),
        crate::model::InstanceKind::Remote => ("server", "remote_server"),
    }
}

#[component]
fn InstanceTypeBadge(kind: crate::model::InstanceKind) -> Element {
    let locale = use_context::<Signal<AppState>>().read().locale;
    let (icon, label) = type_identity(kind);
    rsx! {span {class:"instance-kind", Icon {kind:icon} {locale.text(label)}}}
}

#[component]
fn InstanceTypePicker(id: Uuid, kind: crate::model::InstanceKind, disabled: bool) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let locale = state.read().locale;
    rsx! {div {class:"field",label {r#for:"instance-kind",{locale.text("instance_type")}}
        Dropdown {id:"instance-kind",label:locale.text("instance_type"),value:match kind {crate::model::InstanceKind::Local=>"local",crate::model::InstanceKind::LibreTranslate=>"cloud",crate::model::InstanceKind::Remote=>"remote"},choices:vec![Choice{icon:Some("computer".into()),..Choice::new("local",locale.text("local_instance"))},Choice{icon:Some("cloud".into()),..Choice::new("cloud","LibreTranslate.com")},Choice{icon:Some("server".into()),..Choice::new("remote",locale.text("remote_server"))}],disabled,
            onchange:move |value:String|state.write().edit(id,|draft|{let previous_id=draft.id;let port=draft.port;*draft=match value.as_str(){"cloud"=>Instance::hosted(),"remote"=>Instance{name:locale.text("remote_server").into(),..Instance::remote()},_=>Instance{name:locale.text("new_name").into(),..Instance::default()}};draft.id=previous_id;draft.port=port;})}
    }}
}

#[component]
fn RemoteDetails(id: Uuid) -> Element {
    let mut tab = use_signal(|| "translator".to_string());
    let mut state = use_context::<Signal<AppState>>();
    let s = state.read();
    let locale = s.locale;
    let Some(view) = s.views.iter().find(|view| view.instance.id == id).cloned() else {
        return rsx! {};
    };
    let draft = s
        .editor
        .as_ref()
        .filter(|instance| instance.id == id)
        .cloned()
        .unwrap_or_else(|| view.instance.clone());
    rsx! {section {class:"instance-details",
        header {class:"instance-header",
            div {class:"instance-identity",
                div {class:"instance-heading-text",h1 {"{view.instance.name}"} span {class:"status-badge {status_class(view.status)}",span {class:"status-dot {status_class(view.status)}"}{locale.status(view.status)}}}
                div {class:"endpoint-row",code {"{view.instance.url()}"} Button {class:"endpoint-copy",aria_label:locale.text("copy_url"),title:locale.text("copy_url"),onclick:move |_|{let url=state.read().views.iter().find(|view|view.instance.id==id).map(|view|view.instance.url());if let Some(url)=url{state.write().copy(url);}},Icon {kind:"copy"}} InstanceTypeBadge {kind:view.instance.kind}}
            }
            div {class:"instance-tools",Button {class:"instance-refresh",disabled:s.pending,aria_label:locale.text("remote_check_now"),title:locale.text("remote_check_now"),onclick:move |_|state.write().send(Action::CheckConnection(id)),Icon {kind:"refresh"}} Button {class:"delete-instance",disabled:s.pending,aria_label:locale.text("remove_instance"),title:locale.text("remove_instance"),onclick:move |_|state.write().remove=Some(id),Icon {kind:"trash"}}}
        }
        p {class:"field-hint",{locale.text("remote_privacy").replace("{url}",&view.instance.url())}}
        Tabs {class:"workspace-tabs",value:Some(tab()),horizontal:true,on_value_change:move |value|tab.set(value),
            TabList {class:"tab-list",
                TabTrigger {class:"tab-trigger",index:0usize,value:"translator",id:"hosted-translator-tab",Icon {kind:"language"}{locale.text("translator")}}
                TabTrigger {class:"tab-trigger",index:1usize,value:"configuration",id:"hosted-configuration-tab",Icon {kind:"options"}{locale.text("configuration")}}
            }
            TabContent {class:"tab-panel",index:0usize,value:"translator",aria_labelledby:"hosted-translator-tab",if tab()=="translator" {crate::translator::TranslatorPanel {key:"{id}",instance:view.instance.clone(),status:view.status}}}
            TabContent {class:"tab-panel",index:1usize,value:"configuration",aria_labelledby:"hosted-configuration-tab",if tab()=="configuration" {RemoteForm {instance:draft,creating:false,editable:!s.pending||s.saving.is_some()}}}
        }
    }}
}

#[component]
fn RemoteForm(instance: Instance, creating: bool, editable: bool) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let mut show_api_key = use_signal(|| false);
    let s = state.read();
    let locale = s.locale;
    let id = instance.id;
    let key_required = instance.is_hosted();
    let key_label = if key_required {
        "api_key"
    } else {
        "hosted_api_key"
    };
    let error = s.form_error(&instance);
    let visible_error = error.filter(|key| {
        !(creating && *key == "validation_remote_url" && instance.remote_url.trim().is_empty())
    });
    let candidate = instance.clone();
    let key_test_instance = instance.clone();
    let public_instance = instance.clone();
    rsx! {form {class:"instance-form",onsubmit:move |event|{event.prevent_default();if creating {state.write().save_instance(candidate.clone());}},
        fieldset {div {class:"form-grid",
            div {class:if creating {"form-identity-row form-identity-creating"}else{"form-identity-row"},
                if creating {InstanceTypePicker {id,kind:instance.kind,disabled:!editable}}
                div {class:"field",label {r#for:"hosted-name",{locale.text("name")}} Input {id:"hosted-name",name:"name",disabled:!editable,value:instance.name,maxlength:"64",required:true,oninput:move |event:FormEvent|state.write().edit(id,|draft|draft.name=event.value())}}
            }
            if instance.kind == crate::model::InstanceKind::Remote {
                if creating {PublicInstancePicker {instance:public_instance,editable}}
                div {class:"field field-wide",label {r#for:"remote-url",{locale.text("remote_url")}} Input {id:"remote-url",name:"url",r#type:"url",dir:"ltr",disabled:!editable,value:instance.remote_url,required:true,placeholder:"https://translate.example.com",aria_describedby:"remote-url-hint",oninput:move |event:FormEvent|state.write().edit(id,|draft|draft.remote_url=event.value())}
                    p {id:"remote-url-hint",class:"field-hint",{locale.text("remote_url_hint")}}
                }
            }
            div {class:"field field-wide",
                div {class:"api-key-heading",label {r#for:"hosted-api-key",{locale.text(key_label)}}
                    if instance.kind == crate::model::InstanceKind::LibreTranslate {Button {r#type:"button",class:"text-button",onclick:move |_|{let _=webbrowser::open("https://portal.libretranslate.com/");},Icon {kind:"external"}{locale.text("hosted_get_key")}}}
                }
                div {class:if key_required {"api-key-controls"}else{"api-key-controls single"},
                    div {class:"api-key-input",
                        Input {id:"hosted-api-key",name:"api_key",r#type:if show_api_key() {"text"}else{"password"},autocomplete:"off",dir:"ltr",disabled:!editable,required:key_required,value:instance.server.api_key,maxlength:"512",oninput:move |event:FormEvent|state.write().edit(id,|draft|draft.server.api_key=event.value())}
                        Button {r#type:"button",class:"api-key-visibility",disabled:!editable,aria_label:locale.text(if show_api_key() {"hide_api_key"}else{"show_api_key"}),title:locale.text(if show_api_key() {"hide_api_key"}else{"show_api_key"}),aria_pressed:show_api_key(),aria_controls:"hosted-api-key",onclick:move |_|show_api_key.toggle(),
                            Icon {kind:if show_api_key() {"eye-off"}else{"eye"}}
                        }
                    }
                    if key_required {HostedApiKeyTest {instance:key_test_instance,editable}}
                }
            }
            div {class:"health-settings-row",
                div {class:"field",label {r#for:"health-interval",{locale.text("health_interval")}} Input {id:"health-interval",r#type:"number",min:"5",max:"3600",required:true,disabled:!editable,value:"{instance.health.interval_secs}",oninput:move |event:FormEvent|state.write().edit(id,|draft|draft.health.interval_secs=event.value().parse().unwrap_or_default())}}
                div {class:"field",label {r#for:"health-timeout",{locale.text("health_timeout")}} Input {id:"health-timeout",r#type:"number",min:"1",max:"60",required:true,disabled:!editable,value:"{instance.health.timeout_secs}",oninput:move |event:FormEvent|state.write().edit(id,|draft|draft.health.timeout_secs=event.value().parse().unwrap_or_default())}}
                div {class:"field",label {r#for:"health-failures",{locale.text("health_failures")}} Input {id:"health-failures",r#type:"number",min:"1",max:"10",required:true,disabled:!editable,value:"{instance.health.failure_threshold}",oninput:move |event:FormEvent|state.write().edit(id,|draft|draft.health.failure_threshold=event.value().parse().unwrap_or_default())}}
            }
        }}
        if let Some(key)=visible_error {p {class:"field-error",role:"status",{locale.text(key)}}}
        if creating {div {class:"form-actions",
            Button {r#type:"submit",class:"primary",disabled:!editable||error.is_some(),{locale.text("create_instance")}}
            Button {r#type:"button",class:"secondary",onclick:move |_|{let mut s=state.write();s.adding=false;s.editor=None;},{locale.text("cancel")}}
        }}else if error.is_none(){if let Some(status)=s.autosave_status(id){p {class:"autosave-status",role:"status",{locale.text(status)}}}}
    }}
}

#[component]
fn PublicInstancePicker(instance: ReadSignal<Instance>, editable: bool) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let locale = state.read().locale;
    let mut refresh = use_signal(|| 0u32);
    let mut result = use_signal(|| None::<Result<crate::public_instances::Catalog, ()>>);
    let mut probes = use_signal(std::collections::BTreeMap::<String, crate::public_instances::ProbeOutcome>::new);
    use_resource(move || {
        let _ = refresh();
        async move {
            result.set(None);
            probes.write().clear();
            let reply = tokio::task::spawn_blocking(crate::public_instances::fetch).await;
            match reply {
                Ok(Ok(catalog)) => {
                    let candidates = catalog.instances.clone();
                    result.set(Some(Ok(catalog)));
                    crate::public_instances::probe_all(candidates, move |url, outcome| {
                        probes.write().insert(url, outcome);
                    }).await;
                }
                _ => result.set(Some(Err(()))),
            }
        }
    });
    let response = result.read().clone();
    let loading = response.is_none();
    let failed = matches!(response, Some(Err(_)));
    let entries = response.and_then(Result::ok).map(|catalog| catalog.instances).unwrap_or_default();
    let draft = instance();
    let id = draft.id;
    let value = entries.iter().find(|item| item.url == draft.remote_url.trim().trim_end_matches('/'))
        .map(|item| item.url.clone()).unwrap_or_default();
    let mut choices = vec![Choice::new("", locale.text("public_instances_manual"))];
    choices.extend(entries.iter().map(|item| public_instance_choice(item, probes.read().get(&item.url), locale)));
    let completed = probes.read().len();
    let total = entries.len();
    let hint = if loading {"public_instances_loading"} else if failed {"public_instances_failed"}
        else if entries.is_empty() {"public_instances_empty"} else if completed < total {"public_probing"} else {"public_complete"};
    let hint = locale.text(hint).replace("{done}", &completed.to_string()).replace("{total}", &total.to_string());
    rsx! {div {class:"field field-wide public-instances-field",
        label {r#for:"public-instances",{locale.text("public_instances")}}
        div {class:"public-instance-controls",
            Dropdown {id:"public-instances",label:locale.text("public_instances"),value,choices,described_by:"public-instances-hint",disabled:!editable || loading || entries.is_empty(),
                onchange:move |url:String| {
                    state.write().edit(id, |draft| {
                        let previous_name = entries.iter().find(|item| item.url == draft.remote_url).map(|item| item.name.as_str());
                        if let Some(candidate) = entries.iter().find(|item| item.url == url) {
                            candidate.apply(draft, locale.text("remote_server"), previous_name);
                        } else if url.is_empty() {
                            if previous_name.is_some_and(|name| name.chars().take(64).eq(draft.name.chars())) {draft.name = locale.text("remote_server").into();}
                            draft.remote_url.clear();
                            draft.server.api_key.clear();
                        }
                    });
                }
            }
            Button {r#type:"button",class:"secondary tool-button",disabled:!editable || loading,
                title:locale.text("public_instances_reload"),aria_label:locale.text("public_instances_reload"),
                onclick:move |_|refresh += 1,
                if loading {span {class:"spinner",aria_hidden:"true"}} else {Icon {kind:"refresh"}}
            }
        }
        p {id:"public-instances-hint",class:"field-hint",role:"status",aria_live:if completed>0 && completed<total {"off"} else {"polite"},"{hint}"}
        if draft.remote_url.starts_with("http://") {
            p {class:"field-hint",{locale.text("public_instances_http")}}
        }
    }}
}

fn public_instance_choice(candidate: &crate::public_instances::Candidate, probe: Option<&crate::public_instances::ProbeOutcome>, locale: Locale) -> Choice {
    use crate::components::{ChoiceDetails, ChoiceFact, ChoiceStatus};
    use crate::public_instances::ProbeOutcome;
    let codes = candidate.language_codes();
    let formats = candidate.file_formats();
    let count = |values: &Option<Vec<String>>| values.as_ref().map(|values| values.len().to_string()).unwrap_or_else(|| "—".into());
    let reported = |values: &Option<Vec<String>>| values.as_ref().filter(|values| !values.is_empty())
        .map(|values| values.join(", ")).unwrap_or_else(|| locale.text("public_not_reported").into());
    let limit = candidate.character_limit.filter(|value| *value >= -1);
    let limit_label = if limit == Some(-1) {locale.text("unlimited").to_string()}
        else {locale.text("public_chars").replace("{count}", &limit.map(|value| value.to_string()).unwrap_or_else(|| "—".into()))};
    let status = match probe {
        None => ChoiceStatus {label:locale.text("remote_checking").into(),title:locale.text("public_ping").into(),tone:"checking"},
        Some(ProbeOutcome::Online(ms)) => ChoiceStatus {label:format!("{} · {ms} ms",locale.text("remote_online")),title:locale.text("public_ping").into(),tone:"online"},
        Some(ProbeOutcome::Http(code)) => ChoiceStatus {label:format!("HTTP {code}"),title:format!("{} (HTTP {code})",locale.text("public_unreachable")),tone:"offline"},
        Some(ProbeOutcome::Invalid) => ChoiceStatus {label:locale.text("public_unverified").into(),title:locale.text("public_invalid_health").into(),tone:"offline"},
        Some(outcome) => ChoiceStatus {label:locale.text("remote_offline").into(),title:locale.text(if *outcome==ProbeOutcome::Timeout {"public_timeout"} else {"public_unreachable"}).into(),tone:"offline"},
    };
    let title = candidate.checked_at.as_ref().map(|time|format!("{}\n{}",candidate.url,locale.text("public_checked_at").replace("{time}",time))).unwrap_or_else(||candidate.url.clone());
    Choice {details:Some(ChoiceDetails {
        country_code:candidate.country.as_ref().map(|country|country.code.clone()),title,
        metadata:vec![
            ChoiceFact {label:locale.text("public_languages").replace("{count}",&count(&codes)),title:reported(&codes)},
            ChoiceFact {label:if candidate.files_translation==Some(false) {locale.text("public_files_disabled").into()} else {locale.text("public_files").replace("{count}",&count(&formats))},title:if candidate.files_translation==Some(false) {locale.text("public_files_disabled").into()} else {reported(&formats)}},
            ChoiceFact {label:limit_label.clone(),title:if limit.is_none() {locale.text("public_not_reported").into()} else {limit_label}},
        ],status,search_text:format!("{} {} {} {} {}",candidate.name,candidate.url,reported(&codes),reported(&formats),candidate.country.as_ref().map(|country|country.code.as_str()).unwrap_or_default()),
    }),..Choice::new(&candidate.url,&candidate.name)}
}

#[component]
fn HostedApiKeyTest(instance: ReadSignal<Instance>, editable: bool) -> Element {
    let state = use_context::<Signal<AppState>>();
    let locale = state.read().locale;
    let mut generation = use_signal(|| 0u64);
    let mut pending = use_signal(|| false);
    let mut result = use_signal(|| None::<Result<(), crate::translator::ApiError>>);
    use_effect(move || {
        let _ = instance();
        let next = *generation.peek() + 1;
        generation.set(next);
        pending.set(false);
        result.set(None);
    });
    rsx! {div {class:"api-key-test",
        Button {r#type:"button",class:"secondary tool-button",disabled:!editable || pending() || instance().server.api_key.trim().is_empty(),onclick:move |_| {
            if *pending.peek() || !editable || instance.peek().server.api_key.trim().is_empty() {return;}
            let snapshot = instance();
            let revision = *generation.peek();
            pending.set(true);
            result.set(None);
            spawn(async move {
                let reply = tokio::task::spawn_blocking(move || crate::translator::test_api_key(&snapshot))
                    .await.unwrap_or(Err(crate::translator::ApiError::Connection));
                if *generation.peek() == revision {
                    pending.set(false);
                    result.set(Some(reply));
                }
            });
        },
            if pending() {span {class:"spinner",aria_hidden:"true"}}
            {locale.text(if pending() {"hosted_testing_key"} else {"hosted_test_key"})}
        }
        div {class:"api-key-test-result",role:"status",aria_live:"polite",
            if let Some(reply) = result() {
                span {class:if reply.is_ok() {"port-check available"}else{"port-check unavailable"},
                    Icon {kind:if reply.is_ok() {"check"}else{"close"}}
                    {locale.text(match reply {
                        Ok(()) => "hosted_key_valid",
                        Err(crate::translator::ApiError::Authorization) => "hosted_key_invalid",
                        Err(error) => error.key(),
                    })}
                }
            }
        }
    }}
}

#[component]
fn InstanceDetails(id: Uuid) -> Element {
    let mut tab = use_signal(|| "translator".to_string());
    let mut state = use_context::<Signal<AppState>>();
    let s = state.read();
    let locale = s.locale;
    let Some(view) = s.views.iter().find(|v| v.instance.id == id).cloned() else {
        return rsx! {};
    };
    let draft = s
        .editor
        .as_ref()
        .filter(|i| i.id == id)
        .cloned()
        .unwrap_or_else(|| view.instance.clone());
    let busy = view.status.busy();
    let metrics_enabled = draft.server.metrics;
    use_effect(use_reactive!(|metrics_enabled| {
        if !metrics_enabled && tab.peek().as_str() == "statistics" {
            tab.set("translator".into());
        }
    }));
    let any_install = s
        .views
        .iter()
        .any(|v| matches!(v.status, Status::Installing | Status::UpdatingModels));
    let ready = s.active_paths.as_ref().is_some_and(|paths| {
        runtime::selected_ready(&runtime::instance_dir(&paths.root, id), &view.instance)
    });
    let cuda_missing =
        view.instance.device == Device::Cuda && s.gpu.as_ref().is_none_or(|gpu| !gpu.available());
    let idle = !busy && !s.pending && !s.has_unsaved(id);
    let url = view.instance.url();
    let copy_url = url.clone();
    let update_version = s
        .active_paths
        .as_ref()
        .and_then(|paths| runtime::installed_version(&runtime::instance_dir(&paths.root, id)))
        .and_then(|installed| {
            s.releases
                .as_ref()
                .filter(|catalog| crate::releases::newer(&catalog.latest, &installed))
                .map(|catalog| catalog.latest.clone())
        });
    rsx! {
        section { class: "instance-details",
            header { class: "instance-header",
                div { class: "instance-identity",
                    div { class: "instance-heading-text", h1 { "{view.instance.name}" } span { class: "status-badge {status_class(view.status)}", span { class: "status-dot {status_class(view.status)}" } {locale.status(view.status)} } }
                    div { class: "endpoint-row", code { "{url}" } Button { class: "endpoint-copy", aria_label: locale.text("copy_url"), title: locale.text("copy_url"), onclick: move |_| state.write().copy(copy_url.clone()), Icon { kind: "copy" } }
                        InstanceTypeBadge {kind:view.instance.kind}
                        if let Some(pid) = view.pid { span { class: "muted process-id", "PID {pid}" } }
                        if let Some(latest)=update_version {Button {class:"text-button runtime-update-link",onclick:move|_|tab.set("advanced".into()),Icon {kind:"download"} {locale.text("runtime_update_available").replace("{version}",&latest)}}}
                    }
                }
            div { class: "instance-actions",
                if view.status.active() {
                    Button { class: "danger", disabled: s.pending, aria_label: locale.text("stop_server"), title: locale.text("stop_server"), onclick: move |_| state.write().send(Action::Stop(id)), Icon { kind: "stop" } span { class: "action-label", {locale.text("stop_server")} } }
                } else if !ready {
                    Button { class: "primary", disabled: !idle || any_install || cuda_missing, aria_label: locale.text("install_runtime"), title: locale.text("install_runtime"), onclick: move |_| state.write().send(Action::Install(id)), Icon { kind: "download" } span { class: "action-label", {locale.text("install_runtime")} } }
                } else {
                    Button { class: "primary", disabled: !idle || cuda_missing, aria_label: locale.text("start_server"), title: locale.text("start_server"), onclick: move |_| state.write().send(Action::Start(id)), Icon { kind: "play" } span { class: "action-label", {locale.text("start_server")} } }
                }
                if view.status == Status::Running { Button { class: "secondary", aria_label: locale.text("open_translator"), title: locale.text("open_translator"), onclick: move |_| tab.set("translator".into()), Icon { kind: "language" } span { class: "action-label", {locale.text("open_translator")} } } }
            }
                div { class: "instance-tools",
                    if ready { Button { class: "secondary tool-button", disabled: !idle || any_install, aria_label: locale.text("repair_runtime"), title: locale.text("repair_runtime"), onclick: move |_| state.write().send(Action::Install(id)), Icon { kind: "repair" } } }
                    Button { class: "delete-instance", disabled: !idle, aria_label: locale.text("remove_instance"), title: locale.text("remove_instance"), onclick: move |_| state.write().remove = Some(id), Icon { kind: "trash" } }
                }
            }
            if cuda_missing { p { class: "inline-warning", {locale.text("gpu_saved_missing")} } }
            match view.status {
                Status::UpdatingModels => rsx! { p { class: "progress-note", span { class: "spinner", aria_hidden: "true" } {locale.text("models_update_progress")} } },
                Status::Installing => rsx! { p { class: "progress-note", span { class: "spinner", aria_hidden: "true" } {locale.text("install_progress")} } },
                Status::Starting => rsx! { p { class: "progress-note", span { class: "spinner", aria_hidden: "true" } {locale.text("start_progress")} } },
                Status::NotInstalled => rsx! { p { class: "muted status-help", {locale.text("install_hint")} } },
                Status::Unresponsive => rsx! { p { class: "inline-warning", {locale.text("unresponsive_hint")} } },
                Status::Failed => rsx! { p { class: "inline-warning", {locale.text("failed_hint")} } },
                _ => rsx! {},
            }
            Tabs { class: "workspace-tabs", value: Some(tab()), horizontal: true, on_value_change: move |value| tab.set(value),
                TabList { class: "tab-list",
                    TabTrigger {class:"tab-trigger",index:0usize,value:"translator",id:"instance-translator-tab",Icon{kind:"language"}{locale.text("translator")}}
                    TabTrigger { class: "tab-trigger", index: 1usize, value: "configuration", id:"instance-configuration-tab", Icon{kind:"options"}{locale.text("configuration")} }
                    TabTrigger { class: "tab-trigger", index: 2usize, value: "models", id:"instance-models-tab", Icon{kind:"models"}{locale.text("models")} }
                    if metrics_enabled {TabTrigger {class:"tab-trigger",index:3usize,value:"statistics",id:"instance-statistics-tab",Icon{kind:"chart"}{locale.text("statistics")}}}
                    TabTrigger { class: "tab-trigger", index: if metrics_enabled {4usize}else{3usize}, value: "advanced", id:"instance-advanced-tab", Icon{kind:"advanced"}{locale.text("advanced")} }
                    TabTrigger { class: "tab-trigger", index: if metrics_enabled {5usize}else{4usize}, value: "logs", id:"instance-logs-tab", Icon{kind:"logs"}{locale.text("live_log")} }
                }
                TabContent {class:"tab-panel",index:0usize,value:"translator",aria_labelledby:"instance-translator-tab",if tab()=="translator" {crate::translator::TranslatorPanel{key:"{id}",instance:view.instance.clone(),status:view.status}}}
                TabContent { class: "tab-panel", index: 1usize, value: "configuration", aria_labelledby:"instance-configuration-tab", if tab()=="configuration" { InstanceForm { instance: draft.clone(), creating: false, editable: !busy && (!s.pending || s.saving.is_some()) } } }
                TabContent { class: "tab-panel", index: 2usize, value: "models", aria_labelledby:"instance-models-tab", if tab()=="models" { if let Some(paths)=s.active_paths.as_ref() { ModelsPanel { instance:draft.clone(),status:view.status,root:paths.root.clone(),cache:paths.cache.clone() } } } }
                if metrics_enabled {TabContent {class:"tab-panel",index:3usize,value:"statistics",aria_labelledby:"instance-statistics-tab",if tab()=="statistics" {crate::statistics::StatisticsPanel {key:"{id}",instance:view.instance.clone(),status:view.status}}}}
                TabContent { class: "tab-panel", index: if metrics_enabled {4usize}else{3usize}, value: "advanced", aria_labelledby:"instance-advanced-tab", if tab()=="advanced" { AdvancedForm { instance: draft.clone(), editable: !busy && (!s.pending || s.saving.is_some()) } } }
                TabContent { class: "tab-panel", index: if metrics_enabled {5usize}else{4usize}, value: "logs", aria_labelledby:"instance-logs-tab", if tab()=="logs" { LogViewer { id } } }
            }
        }
    }
}

#[component]
fn InstanceForm(instance: Instance, creating: bool, editable: bool) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let s = state.read();
    let locale = s.locale;
    let id = instance.id;
    let available = s.gpu.as_ref().is_some_and(|gpu| gpu.available());
    let thread_limit = crate::model::max_threads();
    let error = s.form_error(&instance);
    let thread_hint = locale
        .text("thread_limit")
        .replace("{max}", &thread_limit.to_string());
    let candidate = instance.clone();
    rsx! {
        form { class: "instance-form", onsubmit: move |event| { event.prevent_default(); if creating { state.write().save_instance(candidate.clone()); } },
            fieldset {
                div { class: "form-grid",
                    div {class:if creating {"form-identity-row form-identity-creating"}else{"form-identity-row"},
                        if creating {InstanceTypePicker {id,kind:instance.kind,disabled:!editable}}
                        div { class: "field field-name", label { r#for: "instance-name", {locale.text("name")} } Input { id: "instance-name", name: "name", disabled: !editable, value: instance.name, maxlength: "64", required: true, oninput: move |event: FormEvent| state.write().edit(id, |draft| draft.name = event.value()) } }
                    }
                    div { class: "field", label { r#for: "instance-port", {locale.text("port")} } Input { id: "instance-port", name: "port", disabled: !editable, r#type: "number", min: "1024", max: "65535", required: true, value: "{instance.port}", oninput: move |event: FormEvent| state.write().edit(id, |draft| draft.port = event.value().parse().unwrap_or_default()) } PortStatus { id, port: instance.port, disabled: !editable } }
                    div { class: "field", label { r#for: "instance-threads", {locale.text("threads")} } Input { id: "instance-threads", name: "threads", disabled: !editable, r#type: "number", min: "1", max: "{thread_limit}", aria_describedby: "thread-limit", required: true, value: "{instance.threads}", oninput: move |event: FormEvent| state.write().edit(id, |draft| draft.threads = event.value().parse().unwrap_or_default()) } p { id: "thread-limit", class: "field-hint", "{thread_hint}" } }
                    div { class: "field field-languages", LanguageRepeater { id, languages: instance.languages, disabled: !editable } }
                    div { class: "field field-device", label { r#for: "instance-device", {locale.text("device")} }
                        Dropdown { id: "instance-device", label: locale.text("device"), value: if instance.device == Device::Cuda { "cuda".to_string() } else { "cpu".to_string() }, choices: vec![Choice::new("cpu", locale.text("cpu")), Choice { icon: Some("cuda".into()), value: "cuda".into(), label: locale.text("cuda").into(), disabled: !available, details: None }], disabled: !editable, onchange: move |value: String| state.write().edit(id, |draft| draft.device = if value == "cuda" { Device::Cuda } else { Device::Cpu }) }
                        GpuSummary {}
                    }
                    NumberSetting { id:"char-limit", label_key:"char_limit", value:instance.server.char_limit, disabled: !editable, onchange:move|value|state.write().edit(id,|draft|draft.server.char_limit=value) }
                    NumberSetting { id:"batch-limit", label_key:"batch_limit", value:instance.server.batch_limit, disabled: !editable, onchange:move|value|state.write().edit(id,|draft|draft.server.batch_limit=value) }
                }
            }
            if !editable { p { class: "field-hint", {locale.text("stop_to_edit")} } }
            if let Some(key) = error { p { class: "field-error", role: "status", {locale.text(key).replace("{max}", &thread_limit.to_string())} } }
            if creating {
                div { class: "form-actions",
                    Button { r#type: "submit", class: "primary", disabled: !editable || error.is_some(), {locale.text("create_instance")} }
                    Button { r#type: "button", class: "secondary", onclick: move |_| { let mut s = state.write(); s.adding = false; s.editor = None; }, {locale.text("cancel")} }
                }
            } else if error.is_none() {
                if let Some(status) = s.autosave_status(id) { p { class: "autosave-status", role: "status", {locale.text(status)} } }
            }
        }
    }
}

#[component]
fn PortStatus(id: Uuid, port: ReadSignal<u16>, #[props(default)] disabled: bool) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let mut refresh = use_signal(|| 0u64);
    use_future(move || async move {
        loop {
            tokio::time::sleep(Duration::from_secs(3)).await;
            *refresh.write() += 1;
        }
    });
    let check = use_resource(move || {
        let requested = port();
        let _ = refresh();
        async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let status = if requested < 1024 {
                network::PortState::Unavailable
            } else {
                tokio::task::spawn_blocking(move || network::probe_port(requested))
                    .await
                    .unwrap_or(network::PortState::Unavailable)
            };
            (requested, status)
        }
    });
    let s = state.read();
    let locale = s.locale;
    let selected = port();
    let reserved = s
        .views
        .iter()
        .find(|view| view.instance.id != id && view.instance.port == selected);
    let own = s.views.iter().any(|view| {
        view.instance.id == id && view.instance.port == selected && view.status.active()
    });
    let probed = check
        .read()
        .as_ref()
        .copied()
        .filter(|(checked, _)| *checked == selected);
    let (class, text, icon) = if selected < 1024 {
        (
            "unavailable",
            locale.text("validation_port").to_string(),
            "close",
        )
    } else if let Some(view) = reserved {
        (
            "unavailable",
            locale
                .text("port_assigned")
                .replace("{name}", &view.instance.name),
            "close",
        )
    } else if own {
        ("available", locale.text("port_own").to_string(), "check")
    } else {
        match probed.map(|(_, status)| status) {
            Some(network::PortState::Available) => (
                "available",
                locale.text("port_available").to_string(),
                "check",
            ),
            Some(network::PortState::InUse) => (
                "unavailable",
                locale.text("port_in_use").to_string(),
                "close",
            ),
            Some(network::PortState::Unavailable) => (
                "unavailable",
                locale.text("port_unavailable").to_string(),
                "close",
            ),
            None => (
                "checking",
                locale.text("port_checking").to_string(),
                "server",
            ),
        }
    };
    let assigned = s
        .views
        .iter()
        .filter(|view| view.instance.id != id)
        .map(|view| view.instance.port)
        .collect::<Vec<_>>();
    rsx! { div { class: "port-check-row",
        span { class: "port-check {class}", role: "status", aria_live: "polite", Icon { kind: icon } "{text}" }
        Button { r#type: "button", class: "text-button port-find", disabled, title: locale.text("find_free_port"), aria_label: locale.text("find_free_port"), onclick: move |_| {
            let assigned = assigned.clone();
            spawn(async move { if let Ok(Some(found)) = tokio::task::spawn_blocking(move || network::find_free_port(5000, &assigned)).await { state.write().edit(id, |draft| { if draft.port == selected { draft.port = found; } }); } });
        }, Icon { kind: "repair" } span { class: "action-label", {locale.text("find_free_port")} } }
    } }
}

#[component]
fn GpuSummary() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let s = state.read();
    let locale = s.locale;
    let info = s.gpu.clone();
    rsx! { div { id: "gpu-note", class: "gpu-summary",
        if let Some(info) = info {
            if info.available() { p { class: "gpu-device", {locale.text("gpu_detected")} ": " {info.devices.join(", ")} } p { class: "field-hint", {locale.text("gpu_note")} } }
            else { p { class: "field-hint", {locale.text(if gpu::supported_platform() { "gpu_missing" } else { "cuda_unsupported" })} } }
        } else { p { class: "field-hint", {locale.text("gpu_checking")} } }
        Button { r#type: "button", class: "secondary gpu-rescan", disabled: s.gpu.is_none(), onclick: move |_| {
            state.write().gpu = None;
            dioxus_core::spawn_forever(async move { let info = tokio::task::spawn_blocking(gpu::detect).await.unwrap_or_default(); state.write().gpu = Some(info); });
        }, if s.gpu.is_none() {span {class:"spinner",aria_hidden:"true"}} else {Icon {kind:"refresh"}} {locale.text(if s.gpu.is_none() {"gpu_checking"} else {"rescan_gpu"})} }
    } }
}

#[component]
fn LogViewer(id: Uuid) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let s = state.read();
    let locale = s.locale;
    let all = s
        .logs
        .get(&id)
        .map(|lines| lines.iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let query = s.filter.to_lowercase();
    let shown = all
        .iter()
        .filter(|line| line.to_lowercase().contains(&query))
        .cloned()
        .collect::<Vec<_>>();
    let copied = all.join("\n");
    let revision = (all.len(), all.last().cloned());
    let mut last = use_signal(|| (0, None::<String>));
    use_effect(move || {
        let s = state.read();
        if s.preferences.follow_logs && *last.peek() != revision {
            last.set(revision.clone());
            document::eval(
                "const log = document.getElementById('log-output'); if (log) log.scrollTop = log.scrollHeight;",
            );
        }
    });
    rsx! { section { class: "log-section", aria_label: locale.text("live_log"),
        div { class: "section-heading", h2 { {locale.text("live_log")} } div { class: "inline-actions",
            Button { class: "text-button", onclick: move |_| state.write().copy(copied.clone()), {locale.text("copy_log")} }
            Button { class: "text-button", onclick: move |_| { state.write().logs.remove(&id); }, {locale.text("clear_log")} }
        } }
        label { class: "sr-only", r#for: "log-filter", {locale.text("filter_log")} }
        Input { id: "log-filter", class: "log-filter", r#type: "search", placeholder: locale.text("filter_log"), value: s.filter.clone(), oninput: move |event: FormEvent| state.write().filter = event.value() }
        pre { id: "log-output", class: "log-output", tabindex: "0", if all.is_empty() { {locale.text("log_empty")} } else { for line in shown { span { class: "log-line {logs::tone(&line).class()}", "{line}\n" } } } }
        p { class: "field-hint", {locale.text("log_limit")} }
    } }
}

#[component]
fn Options() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let s = state.read();
    let locale = s.locale;
    let preferences = s.preferences.clone();
    let active = s
        .active_paths
        .as_ref()
        .map(|paths| paths.root.display().to_string())
        .unwrap_or_default();
    let settings_file = s
        .settings
        .as_ref()
        .map(|settings| settings.file.display().to_string())
        .unwrap_or_default();
    let active_copy = active.clone();
    let settings_copy = settings_file.clone();
    rsx! { section { class: "options-page",
        h1 { {locale.text("options")} }
        section { class: "options-section",
            h2 { {locale.text("interface_language")} }
            label { class: "sr-only", r#for: "options-language", {locale.text("interface_language")} }
            Dropdown { id: "options-language", label: locale.text("interface_language"), value: locale.code().to_string(), choices: locale_choices(), onchange: move |value: String| state.write().set_locale(Locale::from_code(&value)) }
            p { class: "field-hint", {locale.text("language_immediate")} }
            div { class: "switch-row", label { r#for: "follow-logs", {locale.text("follow_logs")} } Switch { id: "follow-logs", class: "dx-switch", checked: Some(preferences.follow_logs), on_checked_change: move |checked| state.write().set_follow_logs(checked), span { class: "switch-thumb" } } }
        }
        section { class: "options-section",
            h2 { {locale.text("appearance")} }
            label { r#for: "theme-choice", {locale.text("theme")} }
            Dropdown { id: "theme-choice", label: locale.text("theme"), value: preferences.theme.code().to_string(), choices: theme_choices(locale), onchange: move |value: String| state.write().set_theme(Theme::from_code(&value)) }
        }
        section { class: "options-section counter-options",
            h2 { {locale.text("translator_counters")} }
            div {class:"switch-row", label {r#for:"counter-characters",{locale.text("counter_characters")}} Switch {id:"counter-characters",class:"dx-switch",checked:Some(preferences.translator_counters.characters),on_checked_change:move |checked| {let mut s=state.write();let mut counters=s.preferences.translator_counters.clone();counters.characters=checked;s.set_translator_counters(counters);},span {class:"switch-thumb"}}}
            div {class:"switch-row", label {r#for:"counter-words",{locale.text("counter_words")}} Switch {id:"counter-words",class:"dx-switch",checked:Some(preferences.translator_counters.words),on_checked_change:move |checked| {let mut s=state.write();let mut counters=s.preferences.translator_counters.clone();counters.words=checked;s.set_translator_counters(counters);},span {class:"switch-thumb"}}}
            div {class:"switch-row", label {r#for:"counter-labels",{locale.text("counter_labels")}} Switch {id:"counter-labels",class:"dx-switch",checked:Some(preferences.translator_counters.labels),disabled:!preferences.translator_counters.characters&&!preferences.translator_counters.words,on_checked_change:move |checked| {let mut s=state.write();let mut counters=s.preferences.translator_counters.clone();counters.labels=checked;s.set_translator_counters(counters);},span {class:"switch-thumb"}}}
            p {class:"field-hint",{locale.text("counter_hint")}}
        }
        section { class: "options-section",
            h2 { {locale.text("storage_title")} }
            if std::env::var_os("LIBRETRANSLATE_GUI_DATA_DIR").is_some() { p { class: "inline-warning", {locale.text("env_override")} } }
            PathField { kind: "data", label_key: "data_folder", hint_key: "data_hint", value: preferences.data_dir }
            PathField { kind: "cache", label_key: "cache_folder", hint_key: "cache_hint", value: preferences.cache_dir }
            PathField { kind: "python", label_key: "python_folder", hint_key: "python_hint", value: preferences.python_dir }
            p { class: "path-note", {locale.text("paths_note")} }
            div { class: "form-actions",
                Button { class: "primary", onclick: move |_| state.write().save_options(), {locale.text("save_options")} }
                Button { class: "secondary", onclick: move |_| { let mut s = state.write(); s.preferences = Preferences::default(); s.locale = s.preferences.locale; }, {locale.text("reset_options")} }
            }
        }
        section { class: "options-section location-info",
            h2 { {locale.text("active_folder")} }
            code { "{active}" } Button { class: "secondary copy-path", onclick: move |_| state.write().copy(active_copy.clone()), Icon { kind: "copy" } {locale.text("copy_path")} }
            h3 { {locale.text("settings_file")} }
            code { "{settings_file}" } Button { class: "secondary copy-path", onclick: move |_| state.write().copy(settings_copy.clone()), Icon { kind: "copy" } {locale.text("copy_path")} }
        }
        section { class: "options-section", h2 { {locale.text("about_title")} } p { {locale.text("about_description")} } p { class: "muted", "LibreTranslate GUI · " {locale.text("version")} " " {env!("CARGO_PKG_VERSION")} }
            div{class:"app-update-check",
                Button{class:"secondary",disabled:s.app_update_checking,onclick:move|_|{let mut s=state.write();s.app_update_refresh=s.app_update_refresh.wrapping_add(1);},Icon{kind:"download"}{locale.text(if s.app_update_checking{"checking_updates"}else{"check_updates"})}}
                if let Some(key)=s.app_update_status {span{class:"field-hint",role:"status",{locale.text(key)}}}
                if s.app_update.is_some(){Button{class:"text-button",onclick:move|_|state.write().app_update_open=true,{locale.text("app_changelog")}}}
            }
            dl {class:"about-details", dt {{locale.text("creator")}} dd {"Neo"} dt {{locale.text("website")}} dd {
                a {class:"about-website", href:"https://github.com/shadersmods/libretranslate-gui", onclick:move |event| {event.prevent_default(); if let Err(error)=webbrowser::open("https://github.com/shadersmods/libretranslate-gui"){state.write().error("operation_failed",error.to_string());}}, "github.com/shadersmods/libretranslate-gui" Icon {kind:"external"} }
            } }
        }
    } }
}

#[component]
fn PathField(
    kind: &'static str,
    label_key: &'static str,
    hint_key: &'static str,
    value: String,
) -> Element {
    let state = use_context::<Signal<AppState>>();
    let locale = state.read().locale;
    let field_id = format!("path-{kind}");
    let hint_id = format!("hint-{kind}");
    let chooser_value = value.clone();
    rsx! { div { class: "field path-field",
        label { r#for: field_id.clone(), {locale.text(label_key)} }
        div { class: "path-input-row",
            Input { id: field_id.clone(), value, placeholder: locale.text("default_path"), aria_describedby: hint_id.clone(), oninput: move |event: FormEvent| update_path(state, kind, event.value()) }
            Button { class: "secondary folder-button", title: locale.text("browse"), aria_label: locale.text("browse"), onclick: move |_| {
                let value = chooser_value.clone();
                spawn(async move {
                    let mut dialog = rfd::AsyncFileDialog::new().set_title(locale.text(label_key));
                    if !value.is_empty() { dialog = dialog.set_directory(value); }
                    if let Some(folder) = dialog.pick_folder().await { update_path(state, kind, folder.path().display().to_string()); }
                });
            }, Icon { kind: "folder" } span { {locale.text("browse")} } }
        }
        p { id: hint_id, class: "field-hint", {locale.text(hint_key)} }
    } }
}

fn update_path(mut state: Signal<AppState>, kind: &str, value: String) {
    let mut s = state.write();
    match kind {
        "data" => s.preferences.data_dir = value,
        "cache" => s.preferences.cache_dir = value,
        "python" => s.preferences.python_dir = value,
        _ => {}
    }
}

#[component]
fn RemoveDialog() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let locale = state.read().locale;
    let remove = state.read().remove;
    let hosted = state
        .read()
        .views
        .iter()
        .any(|view| Some(view.instance.id) == remove && view.instance.is_remote());
    rsx! { DialogRoot { id: "remove-dialog".to_string(), class: "dialog-backdrop", open: Some(remove.is_some()), on_open_change: move |open: bool| { if !open { state.write().remove = None; } },
      DialogContent { class: "remove-dialog",
        DialogTitle { {locale.text("remove_title")} }
        DialogDescription { {locale.text(if hosted {"hosted_remove_description"}else{"remove_description"})} }
        div { class: "form-actions",
            Button { class: "secondary",  onclick: move |_| state.write().remove = None, {locale.text("keep_instance")} }
            Button { class: "danger", onclick: move |_| { if let Some(id) = remove { let mut s = state.write(); s.send(Action::Remove(id)); s.remove = None; s.logs.remove(&id); } }, {locale.text("remove_instance")} }
        }
      }
    } }
}

fn status_class(status: Status) -> &'static str {
    match status {
        Status::Running | Status::RemoteOnline => "running",
        Status::Starting | Status::Installing | Status::UpdatingModels | Status::RemoteChecking => {
            "working"
        }
        Status::Failed | Status::Unresponsive | Status::RemoteOffline => "failed",
        _ => "idle",
    }
}

#[component]
fn Library() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let mut search = use_signal(String::new);
    let s = state.read();
    let locale = s.locale;
    let query = search().to_lowercase();
    let views = s
        .views
        .iter()
        .filter(|view| view.instance.name.to_lowercase().contains(&query))
        .cloned()
        .collect::<Vec<_>>();
    rsx! { section { class: "library",
        div { class: "library-heading", div { h1 { {locale.text("library_title")} } }
            Button { class: "primary", onclick: move |_| state.write().add(), Icon { kind: "plus" } {locale.text("add_instance")} }
        }
        Input { class: "library-search", r#type: "search", aria_label: locale.text("search_instances"), placeholder: locale.text("search_instances"), value: search(), oninput: move |event: FormEvent| search.set(event.value()) }
        div { class: "library-grid",
            for view in views {
                {
                    let id = view.instance.id;
                    let ready = s.active_paths.as_ref().is_some_and(|paths| runtime::selected_ready(&runtime::instance_dir(&paths.root, id), &view.instance));
                    let active = view.status.active();
                    let busy = active || view.status == Status::Installing;
                    let any_install = s.views.iter().any(|v| matches!(v.status, Status::Installing | Status::UpdatingModels));
                    let copy_url = view.instance.url();
                    let card_url = copy_url.trim_start_matches("http://").to_owned();
                    let cuda_missing = view.instance.device == Device::Cuda && !s.gpu.as_ref().is_some_and(|gpu| gpu.available());
                    let (type_icon, type_label) = type_identity(view.instance.kind);
                    rsx! { article { key: "{id}", class: "instance-card",
                        div { class: "card-heading", Button { class: "card-open", aria_label: view.instance.name.clone(), title: view.instance.name.clone(), onclick: move |_| state.write().select(id), InstanceIcon {languages:s.avatar_languages(&view.instance)} h2 { "{view.instance.name}" } }
                            Button { class: "delete-instance", disabled: busy || s.pending, aria_label: format!("{}: {}", locale.text("remove_instance"), view.instance.name), title: locale.text("remove_instance"), onclick: move |_| state.write().remove = Some(id), Icon { kind: "trash" } }
                        }
                        div { class: "card-endpoint", code { "{card_url}" } Button { class: "endpoint-copy", aria_label: format!("{}: {}", locale.text("copy_url"), view.instance.name), title: locale.text("copy_url"), onclick: move |_| state.write().copy(copy_url.clone()), Icon { kind: "copy" } } }
                        InstanceLanguageBadges {languages:s.avatar_languages(&view.instance)}
                        div { class: "card-meta", span { class: "status-badge {status_class(view.status)}", span { class: "status-dot {status_class(view.status)}" } {locale.status(view.status)} }
                            div {class:"card-environment",
                                Button {class:"card-type",r#type:"button",aria_label:locale.text(type_label),title:locale.text(type_label),Icon {kind:type_icon}}
                                if !view.instance.is_remote() {span { class: "card-device", Icon { kind: if view.instance.device == Device::Cuda { "cuda" } else { "cpu" } } {locale.text(if view.instance.device == Device::Cuda { "cuda" } else { "cpu" })} }}
                            }
                        }
                        div { class: "card-actions",
                            Button { class: "secondary", onclick: move |_| state.write().select(id), Icon { kind: "options" } {locale.text("manage")} }
                            if view.instance.is_remote() {Button {class:"primary",onclick:move |_|state.write().select(id),Icon {kind:"language"}{locale.text("translator")}}}
                            else if active { Button { class: "danger", disabled: s.pending, onclick: move |_| state.write().send(Action::Stop(id)), Icon { kind: "stop" } {locale.text("stop_short")} } }
                            else if ready { Button { class: "primary", disabled: s.pending || busy || cuda_missing || s.has_unsaved(id), onclick: move |_| state.write().send(Action::Start(id)), Icon { kind: "play" } {locale.text("start_short")} } }
                            else { Button { class: "primary", disabled: s.pending || busy || any_install || cuda_missing || s.has_unsaved(id), title: locale.text("install_runtime"), onclick: move |_| state.write().send(Action::Install(id)), Icon { kind: "download" } {locale.text("install_short")} } }
                        }
                    } }
                }
            }
        }
        if !query.is_empty() && s.views.iter().all(|v| !v.instance.name.to_lowercase().contains(&query)) { p { class: "muted", {locale.text("no_results")} } }
    } }
}

#[component]
fn InstanceLanguageBadges(languages: String, #[props(default = 8)] max_visible: usize) -> Element {
    let codes: Vec<_> = languages
        .split(',')
        .filter(|code| !code.is_empty())
        .collect();
    rsx! {div {class:"card-languages",
        for code in codes.iter().take(max_visible) {
            span {class:"language-chip",LanguageFlag {code:*code} "{code}"}
        }
        if codes.len()>max_visible {span {class:"language-chip","+{codes.len()-max_visible}"}}
    }}
}

#[derive(serde::Deserialize)]
struct Language {
    code: String,
    name: String,
}
static LANGUAGES: LazyLock<Vec<Language>> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../assets/languages.json"))
        .expect("Invalid language catalog")
});

#[component]
fn LanguageRepeater(id: Uuid, languages: String, disabled: bool) -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let locale = state.read().locale;
    let codes = languages
        .split(',')
        .map(|code| code.trim().to_string())
        .filter(|code| !code.is_empty())
        .collect::<Vec<_>>();
    let next = LANGUAGES
        .iter()
        .find(|language| !codes.contains(&language.code))
        .map(|language| language.code.clone());
    let all_selected = next.is_none();
    let count = codes.len();
    rsx! { div { class: "language-repeater",
        span { class: "field-label", {locale.text("languages")} }
        div { class: "language-rows",
            for (index, code) in codes.iter().cloned().enumerate() {
                {
                    let mut choices = LANGUAGES.iter().map(|language| {
                        let key = format!("language_{}", language.code);
                        let name = locale.text(&key);
                        Choice { icon: Some(language.code.clone()), value: language.code.clone(), label: format!("{} · {}", if name == "[missing translation]" { &language.name } else { name }, language.code), disabled: codes.contains(&language.code) && language.code != code, details: None }
                    }).collect::<Vec<_>>();
                    if !choices.iter().any(|choice| choice.value == code) { choices.push(Choice::new(&code, &code)); }
                    rsx! { div { class: "language-row", key: "{index}",
                        Dropdown { id: format!("language-{index}"), label: format!("{} {}", locale.text("languages"), index + 1), value: code, choices, disabled,
                            onchange: move |value: String| state.write().edit(id, |draft| { let mut selected = draft.languages.split(',').map(str::to_owned).collect::<Vec<_>>(); if let Some(code) = selected.get_mut(index) { *code = value; } draft.languages = selected.join(","); })
                        }
                        Button { r#type: "button", class: "remove-language", disabled: disabled || count <= 2, title: locale.text("remove_language"), aria_label: format!("{} {}", locale.text("remove_language"), index + 1),
                            onclick: move |_| state.write().edit(id, |draft| { let mut selected = draft.languages.split(',').map(str::to_owned).collect::<Vec<_>>(); if selected.len() > 2 && index < selected.len() { selected.remove(index); } draft.languages = selected.join(","); }), Icon { kind: "close" }
                        }
                    } }
                }
            }
        }
        div {class:"language-actions",
            Button { r#type: "button", class: "secondary add-language", disabled: disabled || next.is_none(), onclick: move |_| { if let Some(code) = &next { state.write().edit(id, |draft| { draft.languages.push(','); draft.languages.push_str(code); }); } }, Icon { kind: "plus" } {locale.text("add_language")} }
            Button {r#type:"button",class:"secondary add-language",disabled:disabled||all_selected,onclick:move |_|state.write().edit(id,|draft|{
                let mut selected=draft.languages.split(',').map(str::to_string).collect::<Vec<_>>();
                for language in LANGUAGES.iter(){if !selected.contains(&language.code){selected.push(language.code.clone());}}
                draft.languages=selected.join(",");
            }),Icon{kind:"plus"}{locale.text("add_all_languages")}}
        }
        p { class: "field-hint", {locale.text("language_hint")} }
    } }
}
