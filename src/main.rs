#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod advanced;
mod app;
mod app_updates;
mod components;
mod downloads;
mod gpu;
mod i18n;
mod installation;
mod logs;
mod manager;
mod model;
mod model_ui;
mod models;
mod network;
mod paths;
mod public_instances;
mod releases;
mod remote_health;
mod removals;
mod runtime;
mod self_update;
mod settings;
mod state;
mod statistics;
mod tasks;
mod translator;
mod translator_files;
mod window_chrome;

fn main() {
    if self_update::run_helper_if_requested() {
        return;
    }
    match self_update::recover_if_needed() {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => {
            rfd::MessageDialog::new().set_title("Update recovery failed")
                .set_description(format!("The app could not restore an interrupted update. Its backup has been preserved.\n\n{error:#}"))
                .set_level(rfd::MessageLevel::Error).show();
            return;
        }
    }
    let _ = std::thread::Builder::new()
        .name("update-file-cleanup".into())
        .spawn(|| {
            if let Err(error) = self_update::cleanup_old_stages() {
                eprintln!("Update cleanup deferred: {error:#}");
            }
        });
    use dioxus::desktop::{
        Config, LogicalSize, WindowBuilder,
        tao::event::{Event, WindowEvent},
    };
    use std::sync::{Arc, Mutex};
    let initial = state::AppState::new();
    #[cfg(windows)]
    let native_window = std::sync::Arc::new(std::sync::atomic::AtomicIsize::new(0));
    #[cfg(windows)]
    let native_window_created = native_window.clone();
    let backend = initial.backend.clone();
    let close_requested = initial.close_requested.clone();
    let webview_data = initial
        .active_paths
        .as_ref()
        .filter(|_| initial.fatal.is_none())
        .map(|paths| paths.root.join("webview"))
        .unwrap_or_else(|| std::env::temp_dir().join("libretranslate-desktop-recovery-webview"));
    let initial: state::InitialState = Arc::new(Mutex::new(Some(initial)));
    let image =
        image::open(dioxus_asset_resolver::asset_path(app::ICON).expect("Missing app icon asset"))
            .expect("Invalid app icon asset")
            .into_rgba8();
    let icon = dioxus::desktop::tao::window::Icon::from_rgba(
        image.to_vec(),
        image.width(),
        image.height(),
    )
    .expect("Invalid icon dimensions");
    let config = Config::new()
        .with_custom_head(format!("<link rel=\"icon\" href=\"{}\">", app::icon_uri()))
        .with_data_directory(webview_data)
        .with_window(
            WindowBuilder::new()
                .with_title("LibreTranslate GUI")
                .with_visible(std::env::var_os("LIBRETRANSLATE_GUI_TEST_HIDDEN").is_none())
                .with_decorations(false)
                .with_transparent(true)
                .with_inner_size(LogicalSize::new(1160.0, 820.0))
                .with_min_inner_size(LogicalSize::new(820.0, 600.0)),
        )
        .with_on_window(move |window, _| {
            #[cfg(windows)]
            {
                use dioxus::desktop::tao::platform::windows::WindowExtWindows;
                window.set_undecorated_shadow(false);
                let hwnd = window.hwnd();
                native_window_created.store(hwnd, std::sync::atomic::Ordering::Relaxed);
                window_chrome::borderless(hwnd);
            }
            #[cfg(not(windows))]
            let _ = window;
        })
        .with_icon(icon)
        .with_menu(None)
        // Keep the event loop alive until pending saves are acknowledged. Native
        // close requests hide the window; a failed save makes it visible again.
        .with_close_behaviour(dioxus::desktop::WindowCloseBehaviour::WindowHides)
        .with_background_color((0, 0, 0, 0))
        .with_custom_event_handler(move |event, _| {
            #[cfg(windows)]
            if matches!(
                event,
                Event::WindowEvent {
                    event: WindowEvent::Focused(_)
                        | WindowEvent::Resized(_)
                        | WindowEvent::ScaleFactorChanged { .. },
                    ..
                }
            ) {
                window_chrome::borderless(native_window.load(std::sync::atomic::Ordering::Relaxed));
            }
            if matches!(
                event,
                Event::WindowEvent {
                    event: WindowEvent::CloseRequested,
                    ..
                }
            ) {
                close_requested.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            // Normal close waits for acknowledged saves in the UI. Always stop owned processes.
            if matches!(event, Event::LoopDestroyed)
                && let Ok(mut backend) = backend.lock()
            {
                backend.take();
            }
        });
    dioxus::LaunchBuilder::desktop()
        .with_context(initial)
        .with_cfg(config)
        .launch(app::app);
}

#[cfg(test)]
mod asset_tests {
    #[test]
    fn icon_keeps_the_tile_opaque_and_the_exterior_transparent() {
        let image = image::load_from_memory(include_bytes!("../assets/icon-source.png"))
            .unwrap()
            .into_rgba8();
        assert_eq!(image.get_pixel(0, 0).0[3], 0);
        for y in 200..1000 {
            for x in 300..1000 {
                assert!(
                    image.get_pixel(x, y).0[3] >= 250,
                    "Unexpected hole inside icon at {x},{y}"
                );
            }
        }
    }
}
