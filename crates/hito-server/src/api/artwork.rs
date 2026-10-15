use crate::{
    auth::Account,
    error::{ApiResult, ErrorResponse},
};
use axum::{
    extract::{Path, State},
    response::{IntoResponse, Response},
};
use hito_core::{Error, Services};

#[utoipa::path(get, path = "/api/v1/recordings/{id}/artwork", params(("id" = String, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, description = "Embedded front cover or neighboring raster cover file",
        content(("image/png"), ("image/jpeg"), ("image/gif"), ("image/webp")),
        headers(("Cache-Control" = String, description = "private, no-store"))),
    (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
))]
pub async fn artwork(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let artwork = services
        .catalog
        .artwork(&actor, &id)
        .await?
        .ok_or(Error::NotFound)?;
    Ok((
        [
            ("content-type", artwork.content_type),
            ("cache-control", "private, no-store"),
        ],
        artwork.bytes,
    )
        .into_response())
}
