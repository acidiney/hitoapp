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
    ArtistIdentityChangeReceipt, ArtistIdentityGroup, ArtistIdentityGroupBrowse,
    ArtistIdentityGroupChanges, ArtistIdentityGroupDetail, ArtistIdentityGroupQuery, Page,
    Services, SplitArtistIdentityGroup, UndoArtistIdentityChange,
};
use serde::Serialize;
use std::time::Instant;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Serialize, ToSchema)]
#[serde(transparent)]
pub struct ArtistIdentityGroupPage(pub Page<ArtistIdentityGroup>);

#[utoipa::path(get, path = "/api/v1/artist-identity-groups", operation_id = "artist_identity_groups",
    params(("q" = Option<String>, Query), ("library" = Option<String>, Query),
        ("offset" = Option<i64>, Query), ("limit" = Option<i64>, Query)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistIdentityGroupPage), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_artist_identity_groups", request_id = %Uuid::new_v4()))]
pub async fn list(
    State(services): State<Services>,
    Account(actor): Account,
    QueryParams(query): QueryParams<ArtistIdentityGroupQuery>,
) -> ApiResult<Json<ArtistIdentityGroupPage>> {
    let started = Instant::now();
    let result = services.catalog.artist_identity_groups(&actor, query).await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map(|_| "returned")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist identity groups HTTP request completed"
    );
    Ok(Json(ArtistIdentityGroupPage(result?)))
}

#[utoipa::path(get, path = "/api/v1/artist-identity-groups/{id}", operation_id = "artist_identity_group",
    params(("id" = String, Path), ("library" = Option<String>, Query),
        ("library_offset" = Option<i64>, Query), ("recording_offset" = Option<i64>, Query),
        ("release_offset" = Option<i64>, Query), ("limit" = Option<i64>, Query)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistIdentityGroupDetail), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 422, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_artist_identity_group", request_id = %Uuid::new_v4()))]
pub async fn read(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    QueryParams(query): QueryParams<ArtistIdentityGroupBrowse>,
) -> ApiResult<Json<ArtistIdentityGroupDetail>> {
    let started = Instant::now();
    let result = services
        .catalog
        .artist_identity_group(&actor, &id, query)
        .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map(|_| "returned")
            .unwrap_or_else(|error| error.code()),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist identity group HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(get, path = "/api/v1/artist-identity-groups/{id}/changes", operation_id = "artist_identity_group_changes",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistIdentityGroupChanges), (status = 401, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
    (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "debug", skip_all, fields(operation = "http_artist_identity_changes", request_id = %Uuid::new_v4()))]
pub async fn changes(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Json<ArtistIdentityGroupChanges>> {
    let started = Instant::now();
    let result = services
        .catalog
        .artist_identity_group_changes(&actor, &id)
        .await;
    tracing::debug!(
        outcome = result
            .as_ref()
            .map_or_else(|error| error.code(), |_| "returned"),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist identity changes HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(post, path = "/api/v1/artist-identity-groups/{id}/split", operation_id = "split_artist_identity_group",
    params(("id" = String, Path)), request_body = SplitArtistIdentityGroup,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistIdentityChangeReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "info", skip_all, fields(operation = "http_artist_identity_split", request_id = %Uuid::new_v4()))]
pub async fn split(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<SplitArtistIdentityGroup>,
) -> ApiResult<Json<ArtistIdentityChangeReceipt>> {
    let started = Instant::now();
    let result = services
        .catalog
        .split_artist_identity_group(&actor, &id, request)
        .await;
    tracing::info!(
        outcome = result
            .as_ref()
            .map_or_else(|error| error.code(), |_| "accepted"),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist identity split HTTP request completed"
    );
    Ok(Json(result?))
}

#[utoipa::path(post, path = "/api/v1/artist-identity-groups/{id}/undo", operation_id = "undo_artist_identity_change",
    params(("id" = String, Path)), request_body = UndoArtistIdentityChange,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = ArtistIdentityChangeReceipt), (status = 400, body = ErrorResponse),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 500, body = ErrorResponse),
))]
#[tracing::instrument(level = "info", skip_all, fields(operation = "http_artist_identity_undo", request_id = %Uuid::new_v4()))]
pub async fn undo(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<UndoArtistIdentityChange>,
) -> ApiResult<Json<ArtistIdentityChangeReceipt>> {
    let started = Instant::now();
    let result = services
        .catalog
        .undo_artist_identity_change(&actor, &id, request)
        .await;
    tracing::info!(
        outcome = result
            .as_ref()
            .map_or_else(|error| error.code(), |_| "accepted"),
        elapsed_ms = started.elapsed().as_millis() as u64,
        "Artist identity undo HTTP request completed"
    );
    Ok(Json(result?))
}
