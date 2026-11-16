//! Styled controls adapted from Dioxus Labs' official component registry.
//! See THIRD_PARTY_NOTICES.md for the pinned source and license.
use dioxus::prelude::*;
use dioxus_core::AttributeValue;
use dioxus_primitives::{ContentSide, combobox, tooltip};
mod flags;
mod countries;
mod icons;
pub use icons::Icon;

pub use dioxus_primitives::dialog::{DialogContent, DialogDescription, DialogRoot, DialogTitle};
pub use dioxus_primitives::switch::Switch;
pub use dioxus_primitives::tabs::{TabContent, TabList, TabTrigger, Tabs};

#[derive(Clone, Copy)]
pub struct TooltipController(pub Signal<Option<String>>);

pub fn dismiss_tooltip(mut active: Signal<Option<String>>, id: &str) {
    let is_current = active.peek().as_deref() == Some(id);
    if is_current {
        active.set(None);
    }
}

pub fn position_tooltip(id: String, selector: Option<String>, active: Signal<Option<String>>) {
    let encoded_id = serde_json::to_string(&id).unwrap();
    let selector = serde_json::to_string(&selector).unwrap();
    let mut evaluation = document::eval(&format!(
        "const tooltipId={encoded_id};const tooltipAnchorSelector={selector};\n{}",
        include_str!("../../assets/tooltip.js")
    ));
    spawn(async move {
        if evaluation.recv::<()>().await.is_ok() {
            dismiss_tooltip(active, &id);
        }
    });
}

#[component]
pub fn Button(
    #[props(extends=GlobalAttributes)]
    #[props(extends=button)]
    attributes: Vec<Attribute>,
    onclick: Option<EventHandler<MouseEvent>>,
    onfocus: Option<EventHandler<FocusEvent>>,
    onblur: Option<EventHandler<FocusEvent>>,
    children: Element,
) -> Element {
    let tooltip_id = use_hook(|| format!("tooltip-{}", uuid::Uuid::new_v4()));
    let mut active = use_context::<TooltipController>().0;
    let title = attributes
        .iter()
        .find_map(|attribute| match (&attribute.name, &attribute.value) {
            (&"title", AttributeValue::Text(text)) if !text.is_empty() => Some(text.clone()),
            _ => None,
        });
    let attributes = attributes
        .into_iter()
        .filter(|attribute| attribute.name != "title")
        .collect::<Vec<_>>();
    let focus_id = tooltip_id.clone();
    let blur_id = tooltip_id.clone();
    let titled = title.is_some();
    let button = rsx! { button { "data-tooltip-anchor": tooltip_id.clone(), aria_describedby: title.as_ref().map(|_| tooltip_id.clone()),
        onclick: move |event| { active.set(None); if let Some(callback) = onclick { callback.call(event); } },
        onfocus: move |event| {
            if titled { active.set(Some(focus_id.clone())); }
            if let Some(callback) = onfocus { callback.call(event); }
        },
        onblur: move |event| {
            if titled { dismiss_tooltip(active, &blur_id); }
            if let Some(callback) = onblur { callback.call(event); }
        },
        ..attributes, {children}
    } };
    if let Some(title) = title {
        let anchor_id = tooltip_id.clone();
        let change_id = tooltip_id.clone();
        let open = active.read().as_deref() == Some(tooltip_id.as_str());
        rsx! { tooltip::Tooltip { class: "tooltip-root", open:Some(open), on_open_change:move |open|{if open {active.set(Some(change_id.clone()));}else{dismiss_tooltip(active,&change_id);}},
            tooltip::TooltipTrigger { class: "tooltip-trigger", tabindex: "-1", {button} }
            tooltip::TooltipContent { id: tooltip_id, class: "app-tooltip", side: ContentSide::Top,
                span { onmounted: move |_| position_tooltip(anchor_id.clone(),None,active), "{title}" }
            }
        } }
    } else {
        button
    }
}

#[component]
pub fn Input(
    #[props(extends=GlobalAttributes)]
    #[props(extends=input)]
    attributes: Vec<Attribute>,
    oninput: Option<EventHandler<FormEvent>>,
    onchange: Option<EventHandler<FormEvent>>,
    children: Element,
) -> Element {
    rsx! { input { oninput: move |event| { if let Some(callback) = oninput { callback.call(event); } }, onchange: move |event| { if let Some(callback) = onchange { callback.call(event); } }, ..attributes, {children} } }
}

#[derive(Clone, PartialEq)]
pub struct Choice {
    pub icon: Option<String>,
    pub value: String,
    pub label: String,
    pub disabled: bool,
    pub details: Option<ChoiceDetails>,
}
#[derive(Clone, PartialEq)]
pub struct ChoiceDetails {
    pub country_code: Option<String>,
    pub title: String,
    pub metadata: Vec<ChoiceFact>,
    pub status: ChoiceStatus,
    pub search_text: String,
}
#[derive(Clone, PartialEq)]
pub struct ChoiceFact {
    pub label: String,
    pub title: String,
}
#[derive(Clone, PartialEq)]
pub struct ChoiceStatus {
    pub label: String,
    pub title: String,
    pub tone: &'static str,
}
impl Choice {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        let value = value.into();
        let icon = matches!(value.as_str(), "cpu" | "cuda" | "light" | "dark" | "system")
            .then(|| value.clone())
            .or_else(|| match value.as_str() {
                "local" => Some("server".into()),
                "lan" => Some("globe".into()),
                _ => None,
            });
        Self {
            icon,
            value,
            label: label.into(),
            disabled: false,
            details: None,
        }
    }
    pub fn language(value: impl Into<String>, label: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            icon: Some(value.clone()),
            value,
            label: label.into(),
            disabled: false,
            details: None,
        }
    }
}

#[component]
pub fn Dropdown(
    id: String,
    label: String,
    value: ReadSignal<String>,
    choices: Vec<Choice>,
    onchange: EventHandler<String>,
    #[props(default)] disabled: bool,
    #[props(default)] described_by: Option<String>,
) -> Element {
    let selected = use_memo(move || Some(value()));
    let mut open = use_signal(|| false);
    let mut query = use_signal(String::new);
    let mut pointer_open = use_signal(|| None::<bool>);
    let searchable = choices.len() > 8;
    let locale = use_context::<Signal<crate::state::AppState>>()
        .read()
        .locale;
    let current = choices
        .iter()
        .find(|choice| choice.value == value())
        .cloned();
    let popup_id = format!("{id}-popup");
    let accessible_label = current.as_ref().map_or_else(
        || label.clone(),
        |choice| format!("{label}: {}", choice.label),
    );
    let effect_id = id.clone();
    use_effect(move || {
        if open() {
            let trigger_id = serde_json::to_string(&effect_id).unwrap();
            spawn(async move {
                let mut eval = document::eval(&format!(
                    "const triggerId = {trigger_id};\n{}",
                    include_str!("../../assets/select-popup.js")
                ));
                if eval.recv::<String>().await.is_ok() {
                    open.set(false);
                }
            });
        }
    });
    rsx! { combobox::Combobox::<String> {
        class: "dx-select", value: Some(selected.into()), disabled, open: Some(open()),
        query: Some(query()), on_query_change: move |value| query.set(value),
        on_open_change: move |value: bool| open.set(value),
        on_value_change: move |value: Option<String>| { if let Some(value) = value { onchange.call(value); open.set(false); } },
        button { class: "dx-select-trigger", id, r#type: "button", disabled, aria_label: accessible_label, aria_describedby: described_by, aria_haspopup: "listbox", aria_expanded: open(), aria_controls: popup_id.clone(),
            onmousedown: move |_| pointer_open.set(Some(open())),
            onclick: move |_| { let was_open = pointer_open.take().unwrap_or(open()); query.set(String::new()); open.set(!was_open); },
            onkeydown: move |event| { if matches!(event.key(), Key::ArrowDown | Key::ArrowUp) { query.set(String::new()); open.set(true); event.prevent_default(); } },
            span { class: "selected-option", if let Some(choice) = current { if let Some(details)=choice.details {RichChoiceLabel {label:choice.label,details,compact:true}} else {if let Some(code) = choice.icon { LanguageFlag { code } } span { "{choice.label}" }} } else { "{label}" } }
            Icon { kind: "chevron-down" }
        }
        div { class: "dx-select-popup", id: popup_id, "data-state": if open() { "open" } else { "closed" },
            if open() {
                combobox::ComboboxInput {
                    class: if searchable { "select-search" } else { "select-keyboard-input" },
                    aria_label: if searchable { locale.text("search_options").to_string() } else { label.clone() }, placeholder: locale.text("search_options"),
                }
            }
            combobox::ComboboxList { class: "dx-select-list", aria_label: label.clone(),
                for (index, choice) in choices.into_iter().enumerate() {
                    combobox::ComboboxOption::<String> { key: "{choice.value}", class: if choice.details.is_some() {"dx-select-option rich-select-option"} else {"dx-select-option"}, index, value: choice.value, text_value: choice.details.as_ref().map(|details|details.search_text.clone()).unwrap_or_else(||choice.label.clone()), disabled: choice.disabled,
                        span { class: "select-option-label", if let Some(details)=choice.details {RichChoiceLabel {label:choice.label,details,compact:false}} else {if let Some(code) = choice.icon { LanguageFlag { code } } "{choice.label}"} }
                        combobox::ComboboxItemIndicator { Icon { kind: "check" } }
                    }
                }
                combobox::ComboboxEmpty { class: "select-empty", {locale.text("no_matching_options")} }
            }
        }
    } }
}

#[component]
fn RichChoiceLabel(label: String, details: ChoiceDetails, compact: bool) -> Element {
    let flag = details.country_code.as_deref().and_then(countries::info);
    rsx! {span {class:if compact {"select-rich-option compact"} else {"select-rich-option"},
        if let Some((src, name))=flag {
            img {class:"language-flag country-flag",src,alt:details.country_code.clone().unwrap_or_default(),title:name}
        } else {Icon {kind:"globe"}}
        span {class:"select-rich-content",
            span {class:"select-rich-heading",
                span {class:"select-rich-name",title:details.title,dir:"auto","{label}"}
                span {class:"select-status", "data-tone":details.status.tone,title:details.status.title,"{details.status.label}"}
            }
            if !compact {span {class:"select-rich-facts",
                for fact in details.metadata {span {class:"select-rich-fact",title:fact.title,"{fact.label}"}}
            }}
        }
    }}
}

#[component]
pub fn InstanceIcon(languages: String) -> Element {
    let mut codes = Vec::new();
    for code in languages
        .split(',')
        .map(str::trim)
        .filter(|code| !code.is_empty())
    {
        if !codes.contains(&code) {
            codes.push(code);
        }
    }
    let count = codes.len().min(4);
    let remaining = codes.len().saturating_sub(4);
    rsx! {
        span { class: "instance-icon", aria_hidden: "true",
            if codes.is_empty() {
                Icon { kind: "language" }
            } else {
                span { class: "instance-flag-mosaic", "data-count": "{count}",
                    for code in codes.into_iter().take(4) {
                        span { class: "instance-flag-tile", key: "{code}", LanguageFlag { code: code.to_string() } }
                    }
                }
                if remaining > 0 { span { class: "instance-flag-more", "+{remaining}" } }
            }
        }
    }
}

#[component]
pub fn LanguageFlag(code: String) -> Element {
    if let Some(src) = flags::asset(&code) {
        rsx! { img { class: "language-flag", src, alt: "" } }
    } else {
        rsx! { Icon { kind: code } }
    }
}
