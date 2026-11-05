use crate::{
    auth::Account,
    error::{self, ApiResult, ErrorResponse},
};
use axum::{
    Json,
    body::Body,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use hito_core::{MAX_AVATAR_UPLOAD_BYTES, Services, User};

#[utoipa::path(put, path = "/api/v1/account/avatar", operation_id = "update_avatar",
    request_body(content = Vec<u8>, content_type = "application/octet-stream",
        description = "A JPEG, PNG or WebP picture of at most 2 MiB; the type is read from the bytes"),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = User), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 413, body = ErrorResponse), (status = 422, body = ErrorResponse), (status = 423, body = ErrorResponse),
))]
pub async fn update_avatar(
    State(services): State<Services>,
    Account(actor): Account,
    headers: HeaderMap,
    body: Body,
) -> ApiResult<Response> {
    let declared = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok());
    if declared.is_some_and(|length| length > MAX_AVATAR_UPLOAD_BYTES) {
        return Ok(too_large());
    }
    let Ok(bytes) = axum::body::to_bytes(body, MAX_AVATAR_UPLOAD_BYTES).await else {
        return Ok(too_large());
    };
    let user = services.identity.set_avatar(&actor, bytes.to_vec()).await?;
    Ok(Json(user).into_response())
}

fn too_large() -> Response {
    error::response(
        StatusCode::PAYLOAD_TOO_LARGE,
        "Use a picture of 2 MiB or less",
    )
}

#[utoipa::path(delete, path = "/api/v1/account/avatar", operation_id = "remove_avatar",
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = User), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 423, body = ErrorResponse),
))]
pub async fn remove_avatar(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<User>> {
    Ok(Json(services.identity.remove_avatar(&actor).await?))
}

#[utoipa::path(get, path = "/api/v1/accounts/{id}/avatar", operation_id = "account_avatar",
    params(("id" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, description = "The normalized square JPEG; readable by the account and by administrators",
        content(("image/jpeg")),
        headers(("ETag" = String, description = "The picture version"),
            ("Cache-Control" = String, description = "private, no-cache"))),
    (status = 304, description = "The picture has not changed"),
    (status = 401, body = ErrorResponse), (status = 404, body = ErrorResponse), (status = 423, body = ErrorResponse),
))]
pub async fn account_avatar(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let avatar = services.identity.avatar(&actor, &id).await?;
    let tag = format!("\"{}\"", avatar.version);
    let common = [
        (header::ETAG, tag.clone()),
        (header::CACHE_CONTROL, "private, no-cache".to_owned()),
        (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_owned()),
    ];
    if matches_tag(&headers, &avatar.version) {
        return Ok((StatusCode::NOT_MODIFIED, common).into_response());
    }
    Ok((
        [(header::CONTENT_TYPE, avatar.content_type.to_owned())],
        common,
        avatar.bytes,
    )
        .into_response())
}

fn matches_tag(headers: &HeaderMap, version: &str) -> bool {
    headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.split(',').map(str::trim).any(|candidate| {
                candidate == "*" || candidate.trim_start_matches("W/").trim_matches('"') == version
            })
        })
}
