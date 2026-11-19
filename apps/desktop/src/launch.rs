use crate::exit::Lifecycle;
use futures_util::future::BoxFuture;
use hito_desktop::{
    Lazy, LocalHost, NativePicker, OsControls, Paths, Place, ReturnPort, SystemBrowser,
    SystemMedia, WindowHandle,
};
use hito_playback::{MediaIntegration, media_commands};
use hito_server::{HostPlace, HostPlaceCurrent};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tauri::{AppHandle, Manager, Url};

pub struct Desktop(pub Mutex<Lifecycle>);

#[derive(Clone, Default)]
pub struct Allowed(pub Arc<Mutex<Option<url::Origin>>>);

impl Allowed {
    pub fn permits(&self, url: &Url) -> bool {
        is_app_page(url)
            || self
                .0
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_ref()
                .is_some_and(|origin| *origin == url.origin())
    }

    fn set(&self, origin: Option<url::Origin>) {
        *self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = origin;
    }
}

pub fn is_app_page(url: &Url) -> bool {
    url.scheme() == "tauri" || url.host_str() == Some("tauri.localhost")
}

pub fn first_screen() -> Url {
    #[cfg(windows)]
    let page = "http://tauri.localhost/first-open.html";
    #[cfg(not(windows))]
    let page = "tauri://localhost/first-open.html";
    Url::parse(page).expect("the first screen has a fixed address")
}

pub struct Launch {
    pub data: PathBuf,
    pub music: PathBuf,
    pub web: PathBuf,
    pub return_port: ReturnPort,
    pub window: WindowHandle,
    pub allowed: Allowed,
}

impl Launch {
    pub async fn start(&self, app: &AppHandle) -> hito_desktop::Result<LocalHost> {
        let (sender, commands) = media_commands();
        let window = self.window.clone();
        let controls = Lazy::new(move || OsControls::create(&sender, &window));
        let media = MediaIntegration::new(
            SystemMedia::new(controls, self.data.join("media-artwork")),
            commands,
        );
        let host = LocalHost::start_with_folders(
            Paths {
                data: self.data.clone(),
                music: self.music.clone(),
                web: self.web.clone(),
            },
            Some(media),
            Some(Arc::new(NativePicker::new(app.clone()))),
            Some(Arc::new(SystemBrowser::new(|url| {
                tauri_plugin_opener::open_url(url, None::<&str>).is_ok()
            }))),
            Some(Arc::new(DesktopPlace(app.clone()))),
            self.return_port,
        )
        .await?;
        self.allowed.set(Some(host.entry_url().origin()));
        Ok(host)
    }
}

struct DesktopPlace(AppHandle);

impl HostPlace for DesktopPlace {
    fn current(&self) -> HostPlaceCurrent {
        HostPlaceCurrent::ThisComputer
    }

    fn change(&self) -> BoxFuture<'_, ()> {
        let app = self.0.clone();
        Box::pin(async move {
            tauri::async_runtime::spawn(return_to_first_screen(app));
        })
    }
}

fn lifecycle(app: &AppHandle) -> std::sync::MutexGuard<'_, Lifecycle> {
    app.state::<Desktop>()
        .inner()
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

async fn return_to_first_screen(app: AppHandle) {
    let launch = app.state::<Launch>();
    if Place::forget(&launch.data).is_err() {
        tracing::warn!(
            event = "desktop_place_forget_failed",
            "The saved library choice could not be removed"
        );
    }
    let Some(host) = lifecycle(&app).leave() else {
        return;
    };
    let stopped = host.shutdown().await.is_ok();
    launch.allowed.set(None);
    let exit = lifecycle(&app).finish_leave();
    tracing::info!(
        event = "desktop_place_left",
        stopped,
        "The local library stopped and the first screen opens"
    );
    if exit {
        app.exit(if stopped { 0 } else { 1 });
        return;
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.navigate(first_screen());
    }
}

#[tauri::command]
pub async fn use_this_computer(app: AppHandle) -> Result<(), String> {
    if !lifecycle(&app).begin_start() {
        return Err("busy".into());
    }
    let launch = app.state::<Launch>();
    let host = match launch.start(&app).await {
        Ok(host) => host,
        Err(_) => {
            tracing::warn!(
                event = "desktop_this_computer_failed",
                "The library on this computer could not start"
            );
            if lifecycle(&app).fail_start() {
                app.exit(1);
            }
            return Err("startup".into());
        }
    };
    if Place::ThisComputer.save(&launch.data).is_err() {
        tracing::warn!(
            event = "desktop_place_save_failed",
            "The library choice could not be saved"
        );
    }
    let entry = host.entry_url();
    let leftover = lifecycle(&app).finish_start(host);
    if let Some(host) = leftover {
        let stopped = host.shutdown().await.is_ok();
        *lifecycle(&app) = Lifecycle::Stopped;
        app.exit(if stopped { 0 } else { 1 });
        return Ok(());
    }
    tracing::info!(
        event = "desktop_this_computer_started",
        "The library on this computer started"
    );
    if let Some(window) = app.get_webview_window("main") {
        window
            .navigate(entry)
            .map_err(|_| "navigation".to_string())?;
    }
    Ok(())
}
