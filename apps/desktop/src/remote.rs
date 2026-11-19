use crate::{Error, Result, access::Access};
use axum::{
    Json, Router,
    body::Body,
    extract::{Request, State},
    http::{HeaderMap, HeaderName, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use hito_server::{HostPlace, HostPlaceCurrent, HostPlaceOffer};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use tower_http::services::ServeDir;
use url::Url;

const DROPPED_REQUEST_HEADERS: [HeaderName; 7] = [
    header::HOST,
    header::ORIGIN,
    header::REFERER,
    header::COOKIE,
    header::AUTHORIZATION,
    HeaderName::from_static("x-inertia-version"),
    HeaderName::from_static("proxy-authorization"),
];

const HOP_BY_HOP: [HeaderName; 7] = [
    header::CONNECTION,
    HeaderName::from_static("keep-alive"),
    HeaderName::from_static("proxy-connection"),
    header::TE,
    header::TRAILER,
    header::TRANSFER_ENCODING,
    header::UPGRADE,
];

pub struct RemoteHost {
    entry: Url,
    stop: Option<oneshot::Sender<()>>,
    serving: Option<JoinHandle<std::io::Result<()>>>,
    player: Option<hito_playback::PlayerOwner>,
}

pub trait ServerSession: Send + Sync + 'static {
    fn token(&self) -> Option<String>;
    fn keep(&self, token: &str, username: &str) -> bool;
    fn end(&self);
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Exchange {
    Begin,
    End,
    Other,
}

const SESSION_BODY_LIMIT: usize = 65_536;

struct Forward {
    server: Url,
    client: reqwest::Client,
    session: Arc<dyn ServerSession>,
}

impl RemoteHost {
    #[tracing::instrument(level = "info", skip_all, err(Debug))]
    pub async fn start(
        server: Url,
        web: &Path,
        place: Arc<dyn HostPlace>,
        session: Arc<dyn ServerSession>,
    ) -> Result<Self> {
        Self::launch(server, web, place, session, None).await
    }

    #[cfg(feature = "device")]
    #[tracing::instrument(level = "info", skip_all, err(Debug))]
    pub async fn start_native(
        server: Url,
        web: &Path,
        place: Arc<dyn HostPlace>,
        session: Arc<dyn ServerSession>,
        media: Option<hito_playback::MediaIntegration>,
    ) -> Result<Self> {
        Self::launch(server, web, place, session, Some(media)).await
    }

    async fn launch(
        server: Url,
        web: &Path,
        place: Arc<dyn HostPlace>,
        session: Arc<dyn ServerSession>,
        native: Option<Option<hito_playback::MediaIntegration>>,
    ) -> Result<Self> {
        crate::server_probe::tls_ready();
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(4))
            .retry(reqwest::retry::never())
            .user_agent(crate::user_agent())
            .build()
            .map_err(|_| Error::Startup)?;
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|_| Error::Startup)?;
        let address = listener.local_addr().map_err(|_| Error::Startup)?;
        let origin = Url::parse(&format!("http://{address}")).map_err(|_| Error::Startup)?;
        let (access, entry) = Access::new(&origin);
        let (player_routes, player) = native_player(&server, &client, &session, native);
        let forwarding = Router::new()
            .fallback(forward)
            .with_state(Arc::new(Forward {
                server,
                client,
                session,
            }));
        let files = ServeDir::new(web)
            .append_index_html_on_directories(false)
            .call_fallback_on_method_not_allowed(true)
            .fallback(forwarding);
        let app = access.protect(
            Router::new()
                .route("/api/v1/host/place", get(place_offer))
                .route("/api/v1/host/place/change", post(place_change))
                .route("/api/v1/host/place/forget-me", post(place_forget_me))
                .with_state(place)
                .merge(player_routes)
                .fallback_service(files),
        );
        let (stop, stopped) = oneshot::channel::<()>();
        let serving = tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
        });
        tracing::info!(
            event = "desktop_remote_ready",
            "The window's link to the chosen server started"
        );
        Ok(Self {
            entry,
            stop: Some(stop),
            serving: Some(serving),
            player,
        })
    }

    pub fn entry_url(&self) -> Url {
        self.entry.clone()
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug))]
    pub async fn shutdown(mut self) -> Result<()> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(player) = self.player.take()
            && player.close().await.is_err()
        {
            tracing::warn!(
                event = "desktop_remote_player_close_failed",
                "The native player for the chosen server did not close cleanly"
            );
        }
        let Some(serving) = self.serving.take() else {
            return Ok(());
        };
        match serving.await {
            Ok(Ok(())) => {
                tracing::info!(
                    event = "desktop_remote_stopped",
                    "The window's link to the chosen server stopped"
                );
                Ok(())
            }
            _ => Err(Error::Shutdown),
        }
    }
}

impl Drop for RemoteHost {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

#[cfg(feature = "device")]
fn native_player(
    server: &Url,
    client: &reqwest::Client,
    session: &Arc<dyn ServerSession>,
    native: Option<Option<hito_playback::MediaIntegration>>,
) -> (Router, Option<hito_playback::PlayerOwner>) {
    let Some(media) = native else {
        return (Router::new(), None);
    };
    let remote = Arc::new(crate::remote_player::Server::new(
        client.clone(),
        server.clone(),
        session.clone(),
    ));
    let (player, owner) =
        hito_playback::Player::native_with_seam(crate::remote_player::seam(remote.clone()), media);
    let identity =
        hito_server::PlayerIdentity(Arc::new(crate::remote_player::RemoteIdentity::new(remote)));
    tracing::info!(
        event = "desktop_remote_player_ready",
        "The native player for the chosen server started"
    );
    (
        hito_server::native_player_router(player, identity),
        Some(owner),
    )
}

#[cfg(not(feature = "device"))]
fn native_player(
    _server: &Url,
    _client: &reqwest::Client,
    _session: &Arc<dyn ServerSession>,
    _native: Option<Option<hito_playback::MediaIntegration>>,
) -> (Router, Option<hito_playback::PlayerOwner>) {
    (Router::new(), None)
}

async fn place_offer(State(place): State<Arc<dyn HostPlace>>) -> Json<HostPlaceOffer> {
    Json(HostPlaceOffer {
        changeable: true,
        current: Some(place.current()),
    })
}

async fn place_change(State(place): State<Arc<dyn HostPlace>>) -> StatusCode {
    place.change().await;
    tracing::info!(
        event = "host_place_change_requested",
        "The app was asked to return to its first screen"
    );
    StatusCode::NO_CONTENT
}

async fn place_forget_me(State(place): State<Arc<dyn HostPlace>>) -> StatusCode {
    place.forget_me().await;
    StatusCode::NO_CONTENT
}

pub fn current_server(origin: &Url, remembered: Option<String>) -> HostPlaceCurrent {
    HostPlaceCurrent::Server {
        origin: origin.origin().ascii_serialization(),
        remembered,
    }
}

async fn forward(State(forward): State<Arc<Forward>>, request: Request) -> Response {
    let started = std::time::Instant::now();
    let (parts, body) = request.into_parts();
    let mut target = forward.server.clone();
    target.set_path(parts.uri.path());
    target.set_query(parts.uri.query());
    let mut headers = parts.headers;
    for name in DROPPED_REQUEST_HEADERS.iter().chain(HOP_BY_HOP.iter()) {
        headers.remove(name);
    }
    let method = parts.method;
    let exchange = match (method.as_str(), parts.uri.path()) {
        ("POST", "/api/v1/login" | "/api/v1/setup") => Exchange::Begin,
        ("POST", "/api/v1/logout") => Exchange::End,
        _ => Exchange::Other,
    };
    if let Some(token) = forward.session.token()
        && let Ok(value) = format!("Bearer {token}").parse()
    {
        headers.insert(header::AUTHORIZATION, value);
    }
    let sent = forward
        .client
        .request(method.clone(), target)
        .headers(headers)
        .body(reqwest::Body::wrap_stream(body.into_data_stream()))
        .send()
        .await;
    if exchange == Exchange::End {
        forward.session.end();
        tracing::info!(
            event = "desktop_remote_signed_out",
            "The session for the chosen server was removed from this computer"
        );
    }
    let answer = match sent {
        Ok(answer) => answer,
        Err(_) => {
            tracing::warn!(
                event = "desktop_remote_unreachable",
                method = %method,
                elapsed_ms = started.elapsed().as_millis() as u64,
                "The chosen server did not answer"
            );
            return (StatusCode::BAD_GATEWAY, "Hito could not reach your server.").into_response();
        }
    };
    tracing::debug!(
        event = "desktop_remote_forwarded",
        method = %method,
        status = answer.status().as_u16(),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "A request reached the chosen server"
    );
    if exchange == Exchange::Begin && answer.status().is_success() {
        return begin_session(&forward, answer).await;
    }
    let status = answer.status();
    let headers = answered_headers(answer.headers(), &forward.server);
    let mut response = Response::new(Body::from_stream(answer.bytes_stream()));
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    response
}

async fn begin_session(forward: &Forward, answer: reqwest::Response) -> Response {
    let status = answer.status();
    let headers = answered_headers(answer.headers(), &forward.server);
    let body = match answer.bytes().await {
        Ok(body) if body.len() <= SESSION_BODY_LIMIT => body,
        _ => return not_kept(),
    };
    let Ok(mut session) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return not_kept();
    };
    let token = session
        .get("token")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let username = session
        .pointer("/user/username")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let (Some(token), Some(username)) = (token, username) else {
        return not_kept();
    };
    if !forward.session.keep(&token, &username) {
        tracing::warn!(
            event = "desktop_remote_session_not_kept",
            "The session for the chosen server could not be kept on this computer"
        );
        return not_kept();
    }
    session["token"] = serde_json::Value::String(String::new());
    tracing::info!(
        event = "desktop_remote_signed_in",
        "The session for the chosen server is kept on this computer"
    );
    let mut response = Json(session).into_response();
    *response.status_mut() = status;
    for (name, value) in headers.iter() {
        if name != header::CONTENT_LENGTH {
            response.headers_mut().insert(name, value.clone());
        }
    }
    response
}

fn not_kept() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({
            "error": "This computer couldn't keep you signed in. Try again."
        })),
    )
        .into_response()
}

fn answered_headers(answered: &HeaderMap, server: &Url) -> HeaderMap {
    let mut headers = answered.clone();
    for name in HOP_BY_HOP.iter().chain([header::SET_COOKIE].iter()) {
        headers.remove(name);
    }
    let relative = headers
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| Url::parse(value).ok())
        .filter(|location| location.origin() == server.origin())
        .and_then(|location| {
            let mut path = location.path().to_owned();
            if let Some(query) = location.query() {
                path.push('?');
                path.push_str(query);
            }
            path.parse().ok()
        });
    if let Some(relative) = relative {
        headers.insert(header::LOCATION, relative);
    }
    headers
}
