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
}

struct Forward {
    server: Url,
    client: reqwest::Client,
}

impl RemoteHost {
    #[tracing::instrument(level = "info", skip_all, err(Debug))]
    pub async fn start(server: Url, web: &Path, place: Arc<dyn HostPlace>) -> Result<Self> {
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
        let forwarding = Router::new()
            .fallback(forward)
            .with_state(Arc::new(Forward { server, client }));
        let files = ServeDir::new(web)
            .append_index_html_on_directories(false)
            .call_fallback_on_method_not_allowed(true)
            .fallback(forwarding);
        let app = access.protect(
            Router::new()
                .route("/api/v1/host/place", get(place_offer))
                .route("/api/v1/host/place/change", post(place_change))
                .with_state(place)
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

pub fn current_server(origin: &Url) -> HostPlaceCurrent {
    HostPlaceCurrent::Server {
        origin: origin.origin().ascii_serialization(),
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
    let sent = forward
        .client
        .request(method.clone(), target)
        .headers(headers)
        .body(reqwest::Body::wrap_stream(body.into_data_stream()))
        .send()
        .await;
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
    let status = answer.status();
    let headers = answered_headers(answer.headers(), &forward.server);
    let mut response = Response::new(Body::from_stream(answer.bytes_stream()));
    *response.status_mut() = status;
    *response.headers_mut() = headers;
    response
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
