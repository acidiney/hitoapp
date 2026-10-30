use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::{JsonBody, QueryParams},
};
use axum::{
    Json,
    extract::{Path, State},
};
use hito_core::{
    PromoteArtistCorrection, Services, SharedArtistCorrectionQuery, SharedArtistCorrectionReceipt,
    SharedArtistCorrections, UndoSharedArtistCorrection,
};
use std::time::Instant;
use uuid::Uuid;

#[utoipa::path(get, path = "/api/v1/artist-corrections", operation_id = "shared_artist_corrections",
    params(("offset" = Option<i64>, Query), ("limit" = Option<i64>, Query)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = SharedArtistCorrections), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_shared_artist_corrections", request_id = %Uuid::new_v4()))]
pub async fn list(
    State(services): State<Services>,
    Account(actor): Account,
    QueryParams(query): QueryParams<SharedArtistCorrectionQuery>,
) -> ApiResult<Json<SharedArtistCorrections>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .shared_artist_corrections_page(&actor, query)
        .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map_or_else(|error| error.code(), |_| "returned"),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Shared artist corrections HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(post, path = "/api/v1/artist-corrections/promote", operation_id = "promote_artist_correction",
    request_body = PromoteArtistCorrection, security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = SharedArtistCorrectionReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "info", skip_all, fields(operation = "http_artist_correction_promote", request_id = %Uuid::new_v4()))]
pub async fn promote(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(request): JsonBody<PromoteArtistCorrection>,
) -> ApiResult<Json<SharedArtistCorrectionReceipt>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .promote_artist_correction(&actor, request)
        .await;
    tracing::info!(
        outcome = result
            .as_ref()
            .map_or_else(|error| error.code(), |_| "accepted"),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist correction promotion HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(post, path = "/api/v1/artist-corrections/{id}/undo", operation_id = "undo_shared_artist_correction",
    params(("id" = String, Path)), request_body = UndoSharedArtistCorrection,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = SharedArtistCorrectionReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "info", skip_all, fields(operation = "http_artist_correction_undo", request_id = %Uuid::new_v4()))]
pub async fn undo(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<UndoSharedArtistCorrection>,
) -> ApiResult<Json<SharedArtistCorrectionReceipt>> {
    let started = Instant::now();
    let result = services
        .artist_catalog
        .undo_shared_artist_correction(&actor, &id, request)
        .await;
    tracing::info!(
        outcome = result
            .as_ref()
            .map_or_else(|error| error.code(), |_| "accepted"),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist correction undo HTTP request completed"
    );
    Ok(Json(result?))
}
