mod cli;
mod commands;
mod credentials;
mod engine;
mod error;
mod models;
mod service;
mod store;
mod tray;
mod usage;

use std::time::Duration;

use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_log::{Target, TargetKind};

use service::Service;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .level_for("claude_account_switcher_lib", log::LevelFilter::Debug)
                .targets([
                    Target::new(TargetKind::Stdout),
                    Target::new(TargetKind::LogDir {
                        file_name: Some("app".into()),
                    }),
                ])
                .max_file_size(2_000_000)
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_positioner::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let data_dir = app.path().app_data_dir()?;
            let service = Service::new(app.handle().clone(), data_dir);
            app.manage(service.clone());

            tray::build(app.handle(), service.clone())?;

            if let Some(window) = app.get_webview_window(tray::POPOVER) {
                apply_window_effects(&window);
            }
            // `--show` opens the popover right away (handy on Linux, where the tray icon
            // only offers a menu, and for anyone who launches the app from a shortcut).
            if std::env::args().any(|a| a == "--show") {
                tray::show_popover(app.handle());
            }

            // Background poller: detect the CLI once, then refresh on the configured interval.
            let svc = service.clone();
            tauri::async_runtime::spawn(async move {
                svc.detect_cli().await;
                svc.refresh_all().await;
                loop {
                    let secs = svc.settings().poll_interval_secs.max(30);
                    tokio::time::sleep(Duration::from_secs(secs)).await;
                    svc.refresh_all().await;
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::Focused(false) if window.label() == tray::POPOVER => {
                tray::note_blur_hide();
                let _ = window.hide();
            }
            WindowEvent::CloseRequested { api, .. } if window.label() == tray::POPOVER => {
                api.prevent_close();
                let _ = window.hide();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::refresh_usage,
            commands::add_account,
            commands::cancel_login,
            commands::adopt_current,
            commands::switch_account,
            commands::remove_account,
            commands::reauthenticate,
            commands::update_settings,
            commands::detect_cli,
            commands::open_settings,
            commands::hide_popover,
            commands::resize_popover,
            commands::quit_app,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|_app, event| {
        // A tray app keeps running when its last window closes.
        if let RunEvent::ExitRequested { api, code, .. } = event {
            if code.is_none() {
                api.prevent_exit();
            }
        }
    });
}

fn apply_window_effects(window: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    {
        use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};
        if let Err(e) = apply_vibrancy(
            window,
            NSVisualEffectMaterial::Popover,
            Some(NSVisualEffectState::Active),
            Some(16.0),
        ) {
            log::warn!("vibrancy unavailable: {e}");
        }
    }
    #[cfg(target_os = "windows")]
    {
        if window_vibrancy::apply_mica(window, None).is_err() {
            if let Err(e) = window_vibrancy::apply_acrylic(window, Some((24, 22, 20, 150))) {
                log::warn!("acrylic unavailable: {e}");
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        let _ = window;
    }
}
