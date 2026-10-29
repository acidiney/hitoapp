use crate::{
    config::{Config, InitError},
    labs::{Failure, Labs},
};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::{Next, from_fn_with_state},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use hito_plugin_sdk::{
    ArtistSimilarityPage, ArtistSimilarityRequest, CATALOG_SIMILARITY_VERSION, Capability,
    CatalogSimilarityCapabilities, CatalogSimilarityIdentity, CatalogSimilarityOperation, Manifest,
    PROTOCOL_VERSION,
};
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Instant};
use subtle::ConstantTimeEq;

pub fn router(config: Config) -> Result<Router, InitError> {
    let digest: Arc<[u8; 32]> = Arc::new(Sha256::digest(config.service_bearer.as_bytes()).into());
    Ok(Router::new()
        .route("/v1/manifest", get(manifest))
        .route("/v1/catalog/similarity/capabilities", get(capabilities))
        .route("/v1/catalog/similarity/artists", post(similar))
        .layer(DefaultBodyLimit::max(1024))
        .with_state(Labs::new(config)?)
        .layer(from_fn_with_state(digest, authenticate)))
}

async fn manifest() -> Response {
    private(Json(Manifest {
        id: "org.hito.listenbrainz-similarity".into(),
        name: "ListenBrainz similar artists".into(),
        protocol_version: PROTOCOL_VERSION,
        capabilities: [Capability::Catalog].into(),
        authorization: None,
        content_policy: None,
    }))
}

async fn capabilities() -> Response {
    private(Json(CatalogSimilarityCapabilities {
        similarity_version: CATALOG_SIMILARITY_VERSION,
        operations: vec![CatalogSimilarityOperation::ArtistSimilarity],
        artist_identity: CatalogSimilarityIdentity::MusicbrainzArtist,
    }))
}

async fn similar(
    State(labs): State<Labs>,
    payload: Result<Json<ArtistSimilarityRequest>, JsonRejection>,
) -> Response {
    let started = Instant::now();
    let result = match payload {
        Ok(Json(request)) => labs.similar(&request).await,
        Err(_) => Err(Failure::Invalid),
    };
    tracing::debug!(
        event = "similarity_request_complete",
        outcome = match &result {
            Ok(_) => "success",
            Err(Failure::Invalid) => "invalid_request",
            Err(Failure::Unavailable) => "unavailable",
            Err(Failure::BadResponse) => "invalid_response",
        },
        item_count = result
            .as_ref()
            .map_or(0, |page: &ArtistSimilarityPage| page.items.len()),
        elapsed_ms = started.elapsed().as_millis() as u64
    );
    match result {
        Ok(page) => private(Json(page)),
        Err(failure) => {
            let (status, message) = match failure {
                Failure::Invalid => (
                    StatusCode::BAD_REQUEST,
                    "The similar-artist request is invalid",
                ),
                Failure::Unavailable => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "The ListenBrainz service is unavailable",
                ),
                Failure::BadResponse => (
                    StatusCode::BAD_GATEWAY,
                    "ListenBrainz returned an invalid response",
                ),
            };
            let mut response =
                (status, Json(serde_json::json!({"error": message}))).into_response();
            response.headers_mut().insert(
                "cache-control",
                HeaderValue::from_static("private, no-store"),
            );
            response
        }
    }
}

fn private(body: impl IntoResponse) -> Response {
    let mut response = body.into_response();
    response.headers_mut().insert(
        "cache-control",
        HeaderValue::from_static("private, no-store"),
    );
    response
}

async fn authenticate(
    State(digest): State<Arc<[u8; 32]>>,
    request: Request,
    next: Next,
) -> Response {
    if !authorized(request.headers(), &digest) {
        let mut response = (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "The service credential is missing or invalid"})),
        )
            .into_response();
        response.headers_mut().insert(
            "cache-control",
            HeaderValue::from_static("private, no-store"),
        );
        return response;
    }
    next.run(request).await
}

fn authorized(headers: &HeaderMap, digest: &[u8; 32]) -> bool {
    let mut values = headers.get_all("authorization").iter();
    let Some(token) = values
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .filter(|token| {
            !token.is_empty()
                && token.len() <= 4096
                && token.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        })
    else {
        return false;
    };
    if values.next().is_some() {
        return false;
    }
    let supplied: [u8; 32] = Sha256::digest(token.as_bytes()).into();
    let mut protocol = headers.get_all("x-hito-plugin-protocol").iter();
    bool::from(supplied.ct_eq(digest))
        && protocol.next().is_some_and(|value| value == "1")
        && protocol.next().is_none()
}
