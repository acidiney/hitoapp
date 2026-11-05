use futures_util::future::BoxFuture;
use hito_server::{HostBrowser, HostOpen, HostReturn};
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle, time::Instant};
use url::Url;

pub const DEFAULT_RETURN_PORT: u16 = 47321;
pub const RETURN_PORT_SETTING: &str = "HITO_DESKTOP_OAUTH_PORT";
const IDLE_AFTER: Duration = Duration::from_secs(11 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReturnPort(u16);

#[derive(Debug, thiserror::Error)]
#[error("{RETURN_PORT_SETTING} must be a port number from 1024 to 65535.")]
pub struct InvalidReturnPort;

impl ReturnPort {
    pub fn from_setting(value: Option<&str>) -> Result<Self, InvalidReturnPort> {
        let Some(value) = value else {
            return Ok(Self(DEFAULT_RETURN_PORT));
        };
        match value.trim().parse::<u16>() {
            Ok(port) if port >= 1024 => Ok(Self(port)),
            _ => Err(InvalidReturnPort),
        }
    }

    pub fn any_free_port() -> Self {
        Self(0)
    }

    pub fn number(self) -> u16 {
        self.0
    }
}

impl Default for ReturnPort {
    fn default() -> Self {
        Self(DEFAULT_RETURN_PORT)
    }
}

struct Listening {
    url: Url,
    deadline: Arc<Mutex<Instant>>,
    stop: oneshot::Sender<()>,
    task: JoinHandle<()>,
}

pub(crate) struct LoopbackReturn {
    port: ReturnPort,
    app: axum::Router,
    idle: Duration,
    listening: tokio::sync::Mutex<Option<Listening>>,
}

impl LoopbackReturn {
    pub fn new(port: ReturnPort, app: axum::Router) -> Self {
        Self::with_idle(port, app, IDLE_AFTER)
    }

    fn with_idle(port: ReturnPort, app: axum::Router, idle: Duration) -> Self {
        Self {
            port,
            app,
            idle,
            listening: tokio::sync::Mutex::new(None),
        }
    }

    pub async fn ensure(&self) -> HostReturn {
        let mut listening = self.listening.lock().await;
        if let Some(current) = listening.take() {
            let expired =
                *current.deadline.lock().unwrap_or_else(|p| p.into_inner()) <= Instant::now();
            if !current.task.is_finished() && !expired {
                *current.deadline.lock().unwrap_or_else(|p| p.into_inner()) =
                    Instant::now() + self.idle;
                let url = current.url.clone();
                *listening = Some(current);
                return HostReturn::Ready(url);
            }
            let _ = current.stop.send(());
            let _ = current.task.await;
        }
        match self.bind().await {
            Ok(started) => {
                let url = started.url.clone();
                *listening = Some(started);
                HostReturn::Ready(url)
            }
            Err(message) => HostReturn::Unavailable(message),
        }
    }

    pub async fn close(&self) {
        if let Some(current) = self.listening.lock().await.take() {
            let _ = current.stop.send(());
            let _ = current.task.await;
        }
    }

    async fn bind(&self) -> Result<Listening, String> {
        let wanted = self.port.number();
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, wanted)))
            .await
            .map_err(|error| {
                tracing::warn!(
                    event = "desktop_return_unavailable",
                    port = wanted,
                    error_kind = ?error.kind(),
                    "The provider return listener could not start"
                );
                if error.kind() == std::io::ErrorKind::AddrInUse {
                    format!(
                        "Port {wanted} is in use, so Hito cannot receive the provider's sign-in. Close the program using it, or set {RETURN_PORT_SETTING} to another port and register the new address with the provider."
                    )
                } else {
                    format!(
                        "Hito could not listen on port {wanted} for the provider's sign-in. Set {RETURN_PORT_SETTING} to another port and register the new address with the provider."
                    )
                }
            })?;
        let address = listener.local_addr().map_err(|_| {
            "Hito could not read the address it listens on for the provider's sign-in.".to_string()
        })?;
        let url = Url::parse(&format!(
            "http://{address}/api/v1/plugin-authorization/callback"
        ))
        .map_err(|_| "Hito could not build the provider return address.".to_string())?;
        let deadline = Arc::new(Mutex::new(Instant::now() + self.idle));
        let (stop, stopped) = oneshot::channel::<()>();
        let watched = deadline.clone();
        let app = self.app.clone();
        let task = tokio::spawn(async move {
            let shutdown = async move {
                tokio::select! {
                    _ = stopped => {}
                    _ = idle(watched) => {}
                }
            };
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(shutdown)
                .await;
            tracing::info!(
                event = "desktop_return_stopped",
                "The provider return listener stopped"
            );
        });
        tracing::info!(
            event = "desktop_return_listening",
            port = address.port(),
            "The provider return listener started"
        );
        Ok(Listening {
            url,
            deadline,
            stop,
            task,
        })
    }
}

async fn idle(deadline: Arc<Mutex<Instant>>) {
    loop {
        let at = *deadline.lock().unwrap_or_else(|p| p.into_inner());
        if Instant::now() >= at {
            return;
        }
        tokio::time::sleep_until(at).await;
    }
}

pub(crate) struct DesktopBrowser {
    opener: Arc<dyn HostBrowser>,
    returns: Arc<LoopbackReturn>,
}

impl DesktopBrowser {
    pub fn new(opener: Arc<dyn HostBrowser>, returns: Arc<LoopbackReturn>) -> Self {
        Self { opener, returns }
    }
}

impl HostBrowser for DesktopBrowser {
    fn open(&self, url: Url) -> BoxFuture<'_, HostOpen> {
        self.opener.open(url)
    }

    fn prepare_return(&self) -> BoxFuture<'_, HostReturn> {
        Box::pin(self.returns.ensure())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn closed(url: &Url) -> bool {
        let port = url.port().unwrap();
        tokio::net::TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
            .await
            .is_err()
    }

    async fn ready(returns: &LoopbackReturn) -> Url {
        match returns.ensure().await {
            HostReturn::Ready(url) => url,
            _ => panic!("the listener should start"),
        }
    }

    #[tokio::test]
    async fn an_idle_listener_stops_and_a_new_attempt_starts_it_again() {
        let returns = LoopbackReturn::with_idle(
            ReturnPort::any_free_port(),
            axum::Router::new(),
            Duration::from_millis(300),
        );
        let first = ready(&returns).await;
        assert!(!closed(&first).await);
        tokio::time::sleep(Duration::from_millis(150)).await;
        let extended = ready(&returns).await;
        assert_eq!(first, extended);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(!closed(&first).await, "a new attempt extends the deadline");
        tokio::time::sleep(Duration::from_millis(700)).await;
        assert!(closed(&first).await);
        let restarted = ready(&returns).await;
        assert!(!closed(&restarted).await);
        returns.close().await;
        assert!(closed(&restarted).await);
    }
}
