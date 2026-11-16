use crate::{
    components::{Button, Icon},
    downloads::DownloadPhase,
    manager::Status,
    model_ui::ModelTransfer,
    state::AppState,
};
use dioxus::prelude::*;
use dioxus_primitives::popover::{PopoverContent, PopoverRoot, PopoverTrigger};

#[component]
pub fn TasksDropdown() -> Element {
    let mut state = use_context::<Signal<AppState>>();
    let mut open = use_signal(|| false);
    let s = state.read();
    let locale = s.locale;
    let mut transfers = s
        .views
        .iter()
        .flat_map(|view| {
            s.downloads
                .get(&view.instance.id)
                .into_iter()
                .flat_map(|entries| entries.values())
                .map(|download| {
                    (
                        view.instance.id,
                        view.instance.name.clone(),
                        download.clone(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    transfers.sort_by_key(|(id, _, item)| {
        (
            match item.phase {
                DownloadPhase::Downloading | DownloadPhase::Validating => 0,
                DownloadPhase::Queued => 1,
                DownloadPhase::Failed | DownloadPhase::Interrupted => 2,
                DownloadPhase::Complete => 3,
            },
            *id,
            item.key(),
        )
    });
    let operations = s
        .views
        .iter()
        .filter(|view| {
            matches!(
                view.status,
                Status::Installing | Status::Starting | Status::UpdatingModels
            ) && !transfers
                .iter()
                .any(|(id, _, item)| *id == view.instance.id && item.phase.active())
        })
        .cloned()
        .collect::<Vec<_>>();
    let active_count = transfers
        .iter()
        .filter(|(_, _, item)| item.phase.active())
        .count()
        + operations.len();
    let history = transfers
        .iter()
        .filter(|(_, _, item)| !item.phase.active())
        .count();
    rsx! { PopoverRoot { class:"tasks-root", open:Some(open()), on_open_change:move |value|open.set(value),
        PopoverTrigger { class:if active_count>0 {"tasks-trigger working"}else{"tasks-trigger"}, aria_label:locale.text("tasks"),
            Icon {kind:"download"} span {{locale.text("tasks")}}
            if active_count>0 {span {class:"task-count","{active_count}"}}
        }
        PopoverContent {class:"tasks-popover", inert:(!open()).then_some(""), side:dioxus_primitives::ContentSide::Bottom, align:dioxus_primitives::ContentAlign::End, aria_label:locale.text("tasks"), dir:locale.direction(),
            div {class:"tasks-header", h2 {{locale.text("tasks")}} Button {class:"task-dismiss", aria_label:locale.text("close_tasks"), onclick:move |_|open.set(false), Icon{kind:"close"}}}
            div {class:"tasks-body",
                if active_count==0&&history==0 {p {class:"tasks-empty",{locale.text("tasks_empty")}}}
                if active_count>0 {section {class:"task-group",h3 {{locale.text("tasks_active")}} ul {
                    for view in operations {li {class:"task-row task-operation",span {class:"spinner",aria_hidden:"true"} div {strong {"{view.instance.name}"} span {{locale.status(view.status)}}}}}
                    for (id,name,download) in transfers.iter().filter(|(_,_,item)|item.phase.active()).cloned() {li {class:"task-row",key:"active-{id}-{download.key()}",span {class:"task-instance","{name}"} ModelTransfer{download}}}
                }}}
                if history>0 {section {class:"task-group", div {class:"task-group-heading",h3 {{locale.text("tasks_history")}} Button {class:"text-button",onclick:move |_|state.write().dismiss_downloads(None),{locale.text("tasks_clear")}}} ul {
                    for (id,name,download) in transfers.into_iter().filter(|(_,_,item)|!item.phase.active()) {
                        {let key=download.key();let failed=matches!(download.phase,DownloadPhase::Failed|DownloadPhase::Interrupted);
                        rsx! {li {class:if failed {"task-row task-attention"}else{"task-row"},key:"history-{id}-{key}",
                            div {class:"task-instance-heading",span {class:"task-instance","{name}"} Button {class:"task-dismiss",aria_label:locale.text("tasks_dismiss"),title:locale.text("tasks_dismiss"),onclick:move |_|state.write().dismiss_downloads(Some((id,key.clone()))),Icon{kind:"close"}}}
                            ModelTransfer{download}
                        }}}
                    }
                }}}
            }
        }
    } }
}
