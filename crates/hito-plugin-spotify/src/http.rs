use crate::{Config, InitError, error::Error, public::Client};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request, State, rejection::JsonRejection},
    middleware::{Next, from_fn},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use hito_plugin_sdk::{
    ArtworkRequest, Capability, Manifest, PROTOCOL_VERSION, PlaylistDetailRequest,
    PlaylistItemsRequest, PlaylistPage, ProviderPlaylist,
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
    let client = Arc::new(Client::new(&config)?);
    Ok(Router::new()
        .route("/v1/manifest", get(manifest))
        .route("/v1/artwork", post(artwork))
        .route("/v1/playlists/detail", post(playlist_detail))
        .route("/v1/playlists/items", post(playlist_items))
        .layer(DefaultBodyLimit::max(4096))
        .layer(from_fn(observe))
        .with_state(client))
}

async fn manifest() -> Json<Manifest> {
    Json(Manifest {
        id: "org.hito.spotify".into(),
        name: "Spotify public playlists".into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: BTreeSet::from([Capability::Playlists, Capability::Artwork]),
        authorization: None,
        content_policy: None,
    })
}

async fn artwork(
    State(client): State<Arc<Client>>,
    payload: Result<Json<ArtworkRequest>, JsonRejection>,
) -> Result<Response, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidArtworkRequest)?;
    let artwork = client.artwork(request).await?;
    Ok((
        [
            ("content-type", artwork.content_type),
            ("cache-control", "private, no-store"),
            ("etag", artwork.etag.as_str()),
        ],
        artwork.bytes,
    )
        .into_response())
}

async fn playlist_detail(
    State(client): State<Arc<Client>>,
    payload: Result<Json<PlaylistDetailRequest>, JsonRejection>,
) -> Result<Json<ProviderPlaylist>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    client.read(request).await.map(Json)
}

async fn playlist_items(
    State(client): State<Arc<Client>>,
    payload: Result<Json<PlaylistItemsRequest>, JsonRejection>,
) -> Result<Json<PlaylistPage>, Error> {
    let Json(request) = payload.map_err(|_| Error::InvalidRequest)?;
    client.items(request).map(Json)
}

async fn observe(request: Request, next: Next) -> Response {
    static SEQUENCE: AtomicU64 = AtomicU64::new(1);
    let request_id = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let operation = match request.uri().path() {
        "/v1/manifest" => "manifest",
        "/v1/artwork" => "artwork",
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
            Err(_) if operation == "artwork" => Error::ArtworkDeadline.into_response(),
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
