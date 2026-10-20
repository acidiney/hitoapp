use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::QueryParams,
};
use axum::{
    Json,
    extract::{Path, State},
};
use hito_core::{
    ArtistIdentityGroup, ArtistIdentityGroupBrowse, ArtistIdentityGroupDetail,
    ArtistIdentityGroupQuery, Page, Services,
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
