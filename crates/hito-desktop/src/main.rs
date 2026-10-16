#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod exit;

use exit::{Decision, Lifecycle};
use hito_desktop::{LocalHost, Paths};
use std::sync::Mutex;
use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, webview::NewWindowResponse};

struct Desktop(Mutex<Lifecycle>);

fn main() {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        "hito_desktop=info,hito_server=info,hito_core=info,hito_playback=info,hito_audio=info"
            .into()
    });
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .json()
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
        .try_init();
    tracing::info!(event = "desktop_starting", "Starting Hito desktop");
    let app = tauri::Builder::default()
        .setup(|app| {
            let data = app.path().app_local_data_dir()?;
            let music = app
                .path()
                .audio_dir()
                .or_else(|_| app.path().home_dir().map(|home| home.join("Music")))?;
            let web = app.path().resource_dir()?.join("web");
            #[cfg(debug_assertions)]
            let web = if web.join("assets/app.js").is_file() {
                web
            } else {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../apps/web/dist")
            };
            let host = tauri::async_runtime::block_on(LocalHost::start(Paths {
                data: data.clone(),
                music,
                web,
            }))?;
            let entry = host.entry_url();
            let origin = entry.origin();
            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(entry))
                .title("Hito")
                .inner_size(1440.0, 1000.0)
                .min_inner_size(390.0, 600.0)
                .data_directory(data.join("webview"))
                .on_navigation(move |url| {
                    let allowed = url.origin() == origin;
                    if !allowed {
                        tracing::warn!(
                            event = "desktop_navigation_rejected",
                            "Navigation outside the local application rejected"
                        );
                    }
                    allowed
                })
                .on_new_window(|_, _| NewWindowResponse::Deny)
                .build();
            if let Err(error) = window {
                if tauri::async_runtime::block_on(host.shutdown()).is_err() {
                    tracing::error!(
                        event = "desktop_start_cleanup_failed",
                        "Local startup cleanup failed"
                    );
                }
                return Err(error.into());
            }
            app.manage(Desktop(Mutex::new(Lifecycle::Running(host))));
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
