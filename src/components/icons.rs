use dioxus::prelude::*;

#[component]
pub fn Icon(kind: String) -> Element {
    let source = match kind.as_str() {
        "plus" => asset!("/assets/icons/plus.svg"),
        "options" => asset!("/assets/icons/options.svg"),
        "language" => asset!("/assets/icons/language.svg"),
        "play" => asset!("/assets/icons/play.svg"),
        "stop" => asset!("/assets/icons/stop.svg"),
        "external" => asset!("/assets/icons/external.svg"),
        "server" => asset!("/assets/icons/server.svg"),
        "folder" => asset!("/assets/icons/folder.svg"),
        "chevron" => asset!("/assets/icons/chevron.svg"),
        "sun" => asset!("/assets/icons/sun.svg"),
        "close" => asset!("/assets/icons/close.svg"),
        "minimize" => asset!("/assets/icons/minimize.svg"),
        "maximize" => asset!("/assets/icons/maximize.svg"),
        "restore" => asset!("/assets/icons/restore.svg"),
        "trash" => asset!("/assets/icons/trash.svg"),
        "cpu" => asset!("/assets/icons/cpu.svg"),
        "cuda" => asset!("/assets/icons/cuda.svg"),
        "light" => asset!("/assets/icons/light.svg"),
        "dark" => asset!("/assets/icons/dark.svg"),
        "system" => asset!("/assets/icons/system.svg"),
        "globe" => asset!("/assets/icons/globe.svg"),
        "chevron-down" => asset!("/assets/icons/chevron-down.svg"),
        "check" => asset!("/assets/icons/check.svg"),
        "copy" => asset!("/assets/icons/copy.svg"),
        "download" => asset!("/assets/icons/download.svg"),
        "repair" => asset!("/assets/icons/repair.svg"),
        "swap" => asset!("/assets/icons/swap.svg"),
        "models" => asset!("/assets/icons/models.svg"),
        "chart" => asset!("/assets/icons/chart.svg"),
        "advanced" => asset!("/assets/icons/advanced.svg"),
        "logs" => asset!("/assets/icons/logs.svg"),
        "file" => asset!("/assets/icons/file.svg"),
        "text" => asset!("/assets/icons/text.svg"),
        _ => asset!("/assets/icons/globe.svg"),
    };
    rsx! { span { class: "icon", style: format!("--icon: url(\"{}\")", source), aria_hidden: "true" } }
}
