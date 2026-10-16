use crate::{Config, InitError, client::Client, detail, error::Error, public};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request, State, rejection::JsonRejection},
    http::HeaderMap,
    middleware::{Next, from_fn},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use hito_plugin_sdk::{
    Capability, Manifest, PROTOCOL_VERSION, PlaylistDetailRequest, PlaylistItemsRequest,
    PlaylistPage, ProviderPlaylist,
};
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tracing::Instrument;

pub fn router(config: Config) -> Result<Router, InitError> {
    let public = public::Client::new(&config)?;
    let client = Arc::new(Clients {
        api: Client::new(config)?,
        public,
    });
    Ok(Router::new()
        .route("/v1/manifest", get(manifest))
        .route("/v1/playlists/detail", post(playlist_detail))
        .route("/v1/playlists/items", post(playlist_items))
        .layer(DefaultBodyLimit::max(4096))
        .layer(from_fn(observe))
        .with_state(client))
}

struct Clients {
    api: Client,
    public: public::Client,
}

async fn manifest() -> Json<Manifest> {
    Json(Manifest {
        id: "org.hito.spotify".into(),
        name: "Spotify public playlists".into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: BTreeSet::from([Capability::Playlists]),
        authorization: None,
    })
}

async fn playlist_detail(
    State(client): State<Arc<Clients>>,
    headers: HeaderMap,
    payload: Result<Json<PlaylistDetailRequest>, JsonRejection>,
) -> Result<Json<ProviderPlaylist>, Error> {
    let values = headers.get_all("authorization");
    if values.iter().next().is_none() {
        let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
        return client.public.read(request).await.map(Json);
    }
    if values.iter().count() != 1 {
        return Err(Error::Unauthorized);
    }
    let token = values
        .iter()
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|token| {
            !token.is_empty()
                && token.len() <= 4096
                && token.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        })
        .ok_or(Error::Unauthorized)?;
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    detail::read(&client.api, token, request).await.map(Json)
}

async fn playlist_items(
    State(client): State<Arc<Clients>>,
    payload: Result<Json<PlaylistItemsRequest>, JsonRejection>,
) -> Result<Json<PlaylistPage>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    client.public.items(request).map(Json)
}

async fn observe(request: Request, next: Next) -> Response {
    static SEQUENCE: AtomicU64 = AtomicU64::new(1);
    let request_id = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let operation = match request.uri().path() {
        "/v1/manifest" => "manifest",
        "/v1/playlists/detail" => "playlist_detail",
        "/v1/playlists/items" => "playlist_items",
        _ => "unknown",
    };
    let span = tracing::info_span!("spotify_request", request_id, operation);
    async move {
        let started = Instant::now();
        let response = match tokio::time::timeout(Duration::from_secs(10), next.run(request)).await
        {
            Ok(response) => response,
            Err(_) => Error::Deadline.into_response(),
        };
        tracing::info!(
            event = "spotify_request_complete",
            status = response.status().as_u16(),
            elapsed_ms = started.elapsed().as_millis() as u64
        );
        response
    }
    .instrument(span)
    .await
}
