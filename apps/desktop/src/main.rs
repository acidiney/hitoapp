#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod exit;
mod launch;

use exit::{Decision, Lifecycle};
use hito_desktop::{
    MusicLookup, Place, RETURN_PORT_SETTING, ReturnPort, WindowHandle, music_folder, user_agent,
};
use launch::{Allowed, Desktop, Launch};
use std::sync::Mutex;
use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, webview::NewWindowResponse};
use tracing_subscriber::prelude::*;

fn main() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        "hito_desktop=info,hito_server=info,hito_core=info,hito_playback=info,hito_audio=info"
            .into()
    });
    let _ = tracing_subscriber::registry()
        .with(tracing_subscriber::filter::filter_fn(
            hito_server::application_log,
        ))
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .json()
                .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE),
        )
        .try_init();
    tracing::info!(event = "desktop_starting", "Starting Hito desktop");
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![launch::use_this_computer])
        .setup(|app| {
            let data = app.path().app_local_data_dir()?;
            let music = music_folder(MusicLookup {
                xdg_override: std::env::var_os("XDG_MUSIC_DIR"),
                os_music: app.path().audio_dir().ok(),
                home: app.path().home_dir().ok(),
            })
            .ok_or("The operating system did not report a Music folder")?;
            let bundled = app.path().resource_dir().map(|dir| dir.join("web"));
            #[cfg(debug_assertions)]
            let web = match bundled {
                Ok(web) if web.join("assets/app.js").is_file() => web,
                _ => std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../web/dist"),
            };
            #[cfg(not(debug_assertions))]
            let web = bundled?;
            let return_port =
                ReturnPort::from_setting(std::env::var(RETURN_PORT_SETTING).ok().as_deref())?;
            let window_handle = WindowHandle::default();
            let allowed = Allowed::default();
            app.manage(Launch {
                data: data.clone(),
                music,
                web,
                return_port,
                window: window_handle.clone(),
                allowed: allowed.clone(),
            });
            let host = match Place::load(&data) {
                Some(Place::ThisComputer) => Some(tauri::async_runtime::block_on(
                    app.state::<Launch>().start(app.handle()),
                )?),
                None => None,
            };
            let url = match &host {
                Some(host) => WebviewUrl::External(host.entry_url()),
                None => WebviewUrl::App("first-open.html".into()),
            };
            let window = WebviewWindowBuilder::new(app, "main", url)
                .title("Hito")
                .user_agent(&user_agent())
                .inner_size(1440.0, 1000.0)
                .min_inner_size(390.0, 600.0)
                .data_directory(data.join("webview"))
                .on_navigation(move |url| {
                    let permitted = allowed.permits(url);
                    if !permitted {
                        tracing::warn!(
                            event = "desktop_navigation_rejected",
                            "Navigation outside the local application rejected"
                        );
                    }
                    permitted
                })
                .on_new_window(|_, _| NewWindowResponse::Deny)
                .build();
            #[cfg(windows)]
            if let Ok(window) = &window
                && let Ok(hwnd) = window.hwnd()
            {
                let _ = window_handle.set(hwnd.0 as usize);
            }
            if let Err(error) = window {
                if let Some(host) = host
                    && tauri::async_runtime::block_on(host.shutdown()).is_err()
                {
                    tracing::error!(
                        event = "desktop_start_cleanup_failed",
                        "Local startup cleanup failed"
                    );
                }
                return Err(error.into());
            }
            let lifecycle = match host {
                Some(host) => Lifecycle::Running(host),
                None => Lifecycle::Choosing,
            };
            app.manage(Desktop(Mutex::new(lifecycle)));
            Ok(())
        })
        .build(tauri::generate_context!());
    let Ok(app) = app else {
        tracing::error!(
            event = "desktop_start_failed",
            "Hito desktop could not start"
        );
        std::process::exit(1);
    };
    app.run(|app, event| {
        if let RunEvent::ExitRequested { api, .. } = event {
            let decision = app
                .state::<Desktop>()
                .0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .request();
            if !matches!(decision, Decision::Exit) {
                api.prevent_exit();
            }
            if let Decision::Shutdown(host) = decision {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let success = host.shutdown().await.is_ok();
                    *app.state::<Desktop>()
                        .0
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Lifecycle::Stopped;
                    tracing::info!(event = "desktop_stopped", success, "Hito desktop stopped");
                    app.exit(if success { 0 } else { 1 });
                });
            }
        }
    });
}
