use axum::{
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use futures_util::future::BoxFuture;
use hito_server::{HostBrowser, HostOpen, HostReturn};
use hyper::server::conn::http1;
use hyper_util::{
    rt::{TokioIo, TokioTimer},
    service::TowerToHyperService,
};
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    net::TcpListener,
    sync::{Semaphore, oneshot},
    task::{JoinHandle, JoinSet},
    time::Instant,
};
use url::Url;

pub const DEFAULT_RETURN_PORT: u16 = 47321;
pub const RETURN_PORT_SETTING: &str = "HITO_DESKTOP_OAUTH_PORT";
const IDLE_AFTER: Duration = Duration::from_secs(11 * 60);

#[derive(Clone, Copy)]
struct Limits {
    idle: Duration,
    header_read: Duration,
    connection: Duration,
    connections: usize,
    stop_grace: Duration,
}

impl Limits {
    const DEFAULT: Self = Self {
        idle: IDLE_AFTER,
        header_read: Duration::from_secs(5),
        connection: Duration::from_secs(15),
        connections: 8,
        stop_grace: Duration::from_secs(2),
    };
}

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

    #[cfg(test)]
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
    limits: Limits,
    listening: tokio::sync::Mutex<Option<Listening>>,
}

impl LoopbackReturn {
    pub fn new(port: ReturnPort, app: axum::Router) -> Self {
        Self::with_limits(port, app, Limits::DEFAULT)
    }

    fn with_limits(port: ReturnPort, app: axum::Router, limits: Limits) -> Self {
        Self {
            port,
            app,
            limits,
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
                    Instant::now() + self.limits.idle;
                let url = current.url.clone();
                *listening = Some(current);
                return HostReturn::Ready(url);
            }
            halt(current, self.limits.stop_grace).await;
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
            halt(current, self.limits.stop_grace).await;
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
        let deadline = Arc::new(Mutex::new(Instant::now() + self.limits.idle));
        let (stop, stopped) = oneshot::channel::<()>();
        let watched = deadline.clone();
        let app = self.app.clone().layer(middleware::from_fn_with_state(
            format!("127.0.0.1:{}", address.port()),
            host_guard,
        ));
        let limits = self.limits;
        let task = tokio::spawn(async move {
            serve(listener, app, stopped, watched, limits).await;
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

async fn halt(listening: Listening, grace: Duration) {
    let Listening { stop, mut task, .. } = listening;
    let _ = stop.send(());
    if tokio::time::timeout(grace, &mut task).await.is_err() {
        task.abort();
        let _ = task.await;
    }
}

async fn host_guard(State(host): State<String>, request: Request, next: Next) -> Response {
    let allowed = request
        .headers()
        .get(header::HOST)
        .is_some_and(|value| value.as_bytes() == host.as_bytes());
    if allowed && !request.headers().contains_key(header::ORIGIN) {
        return next.run(request).await;
    }
    tracing::warn!(
        event = "desktop_return_host_rejected",
        "A request with another host name or a cross-origin marker reached the provider return listener"
    );
    (
        StatusCode::FORBIDDEN,
        [
            (header::CACHE_CONTROL, "no-store"),
            (header::REFERRER_POLICY, "no-referrer"),
        ],
    )
        .into_response()
}

async fn serve(
    listener: TcpListener,
    app: axum::Router,
    stopped: oneshot::Receiver<()>,
    deadline: Arc<Mutex<Instant>>,
    limits: Limits,
) {
    let slots = Arc::new(Semaphore::new(limits.connections));
    let mut connections = JoinSet::new();
    let shutdown = async move {
        tokio::select! {
            _ = stopped => {}
            _ = idle(deadline) => {}
        }
    };
    tokio::pin!(shutdown);
    loop {
        tokio::select! {
            biased;
            _ = &mut shutdown => break,
            _ = connections.join_next(), if !connections.is_empty() => {}
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                };
                let Ok(permit) = slots.clone().try_acquire_owned() else {
                    tracing::warn!(
                        event = "desktop_return_busy",
                        "A provider return connection was refused at the connection cap"
                    );
                    continue;
                };
                let service = TowerToHyperService::new(app.clone());
                connections.spawn(async move {
                    let _permit = permit;
                    let mut builder = http1::Builder::new();
                    builder
                        .timer(TokioTimer::new())
                        .header_read_timeout(limits.header_read)
                        .keep_alive(false);
                    let connection = builder.serve_connection(TokioIo::new(stream), service);
                    let _ = tokio::time::timeout(limits.connection, connection).await;
                });
            }
        }
    }
    drop(listener);
    connections.abort_all();
    while connections.join_next().await.is_some() {}
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    const TEST_LIMITS: Limits = Limits {
        idle: Duration::from_millis(300),
        header_read: Duration::from_millis(300),
        connection: Duration::from_secs(2),
        connections: 2,
        stop_grace: Duration::from_millis(500),
    };

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

    async fn stalled(url: &Url) -> tokio::net::TcpStream {
        let mut stream = tokio::net::TcpStream::connect(SocketAddr::from((
            Ipv4Addr::LOCALHOST,
            url.port().unwrap(),
        )))
        .await
        .unwrap();
        stream
            .write_all(b"GET /api/v1/plugin-authorization/callback?sta")
            .await
            .unwrap();
        stream
    }

    #[tokio::test]
    async fn an_idle_listener_stops_and_a_new_attempt_starts_it_again() {
        let returns = LoopbackReturn::with_limits(
            ReturnPort::any_free_port(),
            axum::Router::new(),
            TEST_LIMITS,
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

    #[tokio::test]
    async fn a_stalled_connection_cannot_block_close_or_the_next_connect() {
        let limits = Limits {
            header_read: Duration::from_secs(60),
            connection: Duration::from_secs(60),
            ..TEST_LIMITS
        };
        let returns =
            LoopbackReturn::with_limits(ReturnPort::any_free_port(), axum::Router::new(), limits);
        let url = ready(&returns).await;
        let _stalled = stalled(&url).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let started = Instant::now();
        returns.close().await;
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(closed(&url).await);

        let restarted = ready(&returns).await;
        let _stalled = stalled(&restarted).await;
        tokio::time::sleep(Duration::from_millis(700)).await;
        let started = Instant::now();
        let again = ready(&returns).await;
        assert!(started.elapsed() < Duration::from_secs(2));
        assert!(!closed(&again).await);
        let started = Instant::now();
        returns.close().await;
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn a_connection_that_never_finishes_its_request_line_is_dropped() {
        let returns = LoopbackReturn::with_limits(
            ReturnPort::any_free_port(),
            axum::Router::new(),
            Limits {
                idle: Duration::from_secs(30),
                ..TEST_LIMITS
            },
        );
        let url = ready(&returns).await;
        let mut stream = stalled(&url).await;
        let mut buffer = [0u8; 256];
        let outcome = tokio::time::timeout(Duration::from_secs(3), stream.read(&mut buffer)).await;
        assert!(
            outcome.is_ok(),
            "the server must end the stalled connection"
        );
        returns.close().await;
    }

    #[tokio::test]
    async fn connections_beyond_the_cap_are_refused_while_the_listener_stays_up() {
        let returns = LoopbackReturn::with_limits(
            ReturnPort::any_free_port(),
            axum::Router::new(),
            Limits {
                idle: Duration::from_secs(30),
                header_read: Duration::from_secs(30),
                ..TEST_LIMITS
            },
        );
        let url = ready(&returns).await;
        let _first = stalled(&url).await;
        let _second = stalled(&url).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let mut third = stalled(&url).await;
        let mut buffer = [0u8; 16];
        let refused = tokio::time::timeout(Duration::from_secs(2), third.read(&mut buffer)).await;
        assert!(matches!(refused, Ok(Ok(0)) | Ok(Err(_))));
        returns.close().await;
    }
}
