use crate::exit::Lifecycle;
use futures_util::future::BoxFuture;
use hito_desktop::{
    AppHost, Lazy, LocalHost, NativePicker, Next, OsControls, Outcome, Paths, Place, Prober,
    RecentServers, RemoteHost, ReturnPort, SystemBrowser, SystemMedia, WindowHandle, check,
    current_server,
};
use hito_playback::{MediaIntegration, media_commands};
use hito_server::{HostPlace, HostPlaceCurrent};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
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
    pub attempt: AtomicU64,
}

impl Launch {
    pub async fn start(&self, app: &AppHandle, place: &Place) -> hito_desktop::Result<AppHost> {
        let host = match place {
            Place::ThisComputer => AppHost::Local(self.start_local(app).await?),
            Place::Server { origin } => AppHost::Remote(
                RemoteHost::start(
                    origin.clone(),
                    &self.web,
                    Arc::new(DesktopPlace {
                        app: app.clone(),
                        current: current_server(origin),
                    }),
                )
                .await?,
            ),
        };
        self.allowed.set(Some(host.entry_url().origin()));
        Ok(host)
    }

    async fn start_local(&self, app: &AppHandle) -> hito_desktop::Result<LocalHost> {
        let (sender, commands) = media_commands();
        let window = self.window.clone();
        let controls = Lazy::new(move || OsControls::create(&sender, &window));
        let media = MediaIntegration::new(
            SystemMedia::new(controls, self.data.join("media-artwork")),
            commands,
        );
        LocalHost::start_with_folders(
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
            Some(Arc::new(DesktopPlace {
                app: app.clone(),
                current: HostPlaceCurrent::ThisComputer,
            })),
            self.return_port,
        )
        .await
    }
}

struct DesktopPlace {
    app: AppHandle,
    current: HostPlaceCurrent,
}

impl HostPlace for DesktopPlace {
    fn current(&self) -> HostPlaceCurrent {
        self.current.clone()
    }

    fn change(&self) -> BoxFuture<'_, ()> {
        let app = self.app.clone();
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

async fn open(app: &AppHandle, place: Place) -> Result<(), &'static str> {
    if !lifecycle(app).begin_start() {
        return Err("busy");
    }
    let launch = app.state::<Launch>();
    let host = match launch.start(app, &place).await {
        Ok(host) => host,
        Err(_) => {
            tracing::warn!(
                event = "desktop_place_start_failed",
                remote = matches!(place, Place::Server { .. }),
                "The chosen place could not start"
            );
            if lifecycle(app).fail_start() {
                app.exit(1);
            }
            return Err("startup");
        }
    };
    if place.save(&launch.data).is_err() {
        tracing::warn!(
            event = "desktop_place_save_failed",
            "The library choice could not be saved"
        );
    }
    let entry = host.entry_url();
    let leftover = lifecycle(app).finish_start(host);
    if let Some(host) = leftover {
        let stopped = host.shutdown().await.is_ok();
        *lifecycle(app) = Lifecycle::Stopped;
        app.exit(if stopped { 0 } else { 1 });
        return Ok(());
    }
    tracing::info!(
        event = "desktop_place_started",
        remote = matches!(place, Place::Server { .. }),
        "The chosen place started"
    );
    if let Some(window) = app.get_webview_window("main") {
        window.navigate(entry).map_err(|_| "navigation")?;
    }
    Ok(())
}

#[tauri::command]
pub async fn use_this_computer(app: AppHandle) -> Result<(), String> {
    open(&app, Place::ThisComputer).await.map_err(str::to_owned)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}

#[tauri::command]
pub async fn connect_server(app: AppHandle, address: String, accept_plain: bool) -> Outcome {
    let launch = app.state::<Launch>();
    let attempt = launch.attempt.fetch_add(1, Ordering::SeqCst) + 1;
    let Ok(prober) = Prober::new() else {
        return Outcome::Failed;
    };
    let started = std::time::Instant::now();
    let next = check(
        &prober,
        &address,
        &RecentServers::load(&launch.data),
        accept_plain,
    )
    .await;
    let origin = match next {
        Next::Open(origin) => origin,
        Next::Answer(outcome) => {
            tracing::info!(
                event = "desktop_server_checked",
                outcome = outcome.kind(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "A server address was checked"
            );
            return outcome;
        }
    };
    if launch.attempt.load(Ordering::SeqCst) != attempt {
        return Outcome::Busy;
    }
    match open(
        &app,
        Place::Server {
            origin: origin.clone(),
        },
    )
    .await
    {
        Ok(()) => {
            let mut recent = RecentServers::load(&launch.data);
            if accept_plain {
                recent.accept_plain(&origin, now());
            } else {
                recent.used(&origin, now());
            }
            if recent.save(&launch.data).is_err() {
                tracing::warn!(
                    event = "desktop_recent_save_failed",
                    "The list of recent servers could not be saved"
                );
            }
            Outcome::Opened
        }
        Err("busy") => Outcome::Busy,
        Err(_) => Outcome::Failed,
    }
}

#[tauri::command]
pub fn cancel_connect(app: AppHandle) {
    app.state::<Launch>().attempt.fetch_add(1, Ordering::SeqCst);
}

#[derive(Serialize)]
pub struct Recent {
    origin: String,
    shown: String,
    account: Option<String>,
    used_at: i64,
}

#[tauri::command]
pub fn recent_servers(app: AppHandle) -> Vec<Recent> {
    RecentServers::load(&app.state::<Launch>().data)
        .list()
        .iter()
        .map(|server| Recent {
            origin: server.origin.to_string(),
            shown: hito_desktop::shown(&server.origin),
            account: server.account.clone(),
            used_at: server.used_at,
        })
        .collect()
}

#[tauri::command]
pub fn forget_server(app: AppHandle, origin: String) -> Result<(), String> {
    let origin = Url::parse(&origin).map_err(|_| "address".to_string())?;
    let data = &app.state::<Launch>().data;
    let mut recent = RecentServers::load(data);
    recent.forget(&origin);
    recent.save(data).map_err(|_| "storage".to_string())?;
    if Place::load(data) == Some(Place::Server { origin }) {
        Place::forget(data).map_err(|_| "storage".to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn saved_server(app: AppHandle) -> Option<String> {
    match Place::load(&app.state::<Launch>().data) {
        Some(Place::Server { origin }) => Some(origin.to_string()),
        _ => None,
    }
}
