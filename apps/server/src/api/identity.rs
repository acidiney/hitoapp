use crate::{
    Config,
    auth::Account,
    error::{ApiResult, ErrorResponse},
    extract::JsonBody,
    security,
};
use axum::{
    Extension, Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use hito_core::{AuthSession, Credentials, Services, User};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct SetupStatus {
    pub needs_setup: bool,
}

#[utoipa::path(get, path = "/api/v1/setup", responses(
    (status = 200, body = SetupStatus), (status = 500, body = ErrorResponse),
))]
pub async fn setup_status(State(services): State<Services>) -> ApiResult<Json<SetupStatus>> {
    Ok(Json(SetupStatus {
        needs_setup: services.identity.needs_setup().await?,
    }))
}

#[utoipa::path(post, path = "/api/v1/setup", request_body = Credentials, responses(
    (status = 201, body = AuthSession), (status = 409, body = ErrorResponse),
    (status = 400, body = ErrorResponse), (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 429, body = ErrorResponse),
))]
pub async fn setup(
    State(services): State<Services>,
    Extension(config): Extension<Config>,
    JsonBody(credentials): JsonBody<Credentials>,
) -> ApiResult<Response> {
    let session = services.identity.setup(credentials).await?;
    tracing::info!(event = "setup_completed", "Initial account created");
    Ok(security::session_response(
        &config,
        session,
        StatusCode::CREATED,
    ))
}

#[utoipa::path(post, path = "/api/v1/login", request_body = Credentials, responses(
    (status = 200, body = AuthSession), (status = 401, body = ErrorResponse), (status = 429, body = ErrorResponse),
    (status = 400, body = ErrorResponse), (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse), (status = 422, body = ErrorResponse),
))]
pub async fn login(
    State(services): State<Services>,
    Extension(config): Extension<Config>,
    JsonBody(credentials): JsonBody<Credentials>,
) -> ApiResult<Response> {
    let session = services.identity.login(credentials).await?;
    tracing::info!(event = "login_completed", "Session created");
    Ok(security::session_response(&config, session, StatusCode::OK))
}

#[utoipa::path(post, path = "/api/v1/logout", security(("session" = []), ("bearer" = [])), responses(
    (status = 204), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
))]
pub async fn logout(
    State(services): State<Services>,
    Account(_actor): Account,
    Extension(config): Extension<Config>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    services.identity.logout(security::token(&headers)?).await?;
    tracing::info!(event = "logout_completed", "Session revoked");
    Ok((
        StatusCode::NO_CONTENT,
        [("set-cookie", security::cookie(&config, "", 0))],
    )
        .into_response())
}

#[utoipa::path(get, path = "/api/v1/me", security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = User), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
))]
pub async fn me(Account(actor): Account) -> Json<User> {
    Json(actor.user().clone())
}

#[utoipa::path(get, path = "/api/v1/users", security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = [User]), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
))]
pub async fn users(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<Vec<User>>> {
    Ok(Json(services.identity.users(&actor).await?))
}

#[utoipa::path(post, path = "/api/v1/users", request_body = Credentials,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 201, body = User), (status = 401, body = ErrorResponse),
    (status = 400, body = ErrorResponse), (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 403, body = ErrorResponse), (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse),
))]
pub async fn create_user(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(credentials): JsonBody<Credentials>,
) -> ApiResult<(StatusCode, Json<User>)> {
    Ok((
        StatusCode::CREATED,
        Json(services.identity.create_user(&actor, credentials).await?),
    ))
}

#[utoipa::path(put, path = "/api/v1/libraries/{library}/members/{member}",
    params(("library" = String, Path), ("member" = String, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 204), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse), (status = 404, body = ErrorResponse),
))]
pub async fn grant_access(
    State(services): State<Services>,
    Account(actor): Account,
    Path((library, member)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    services
        .identity
        .set_library_access(&actor, &library, &member, true)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[utoipa::path(delete, path = "/api/v1/libraries/{library}/members/{member}",
    params(("library" = String, Path), ("member" = String, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 204), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
))]
pub async fn revoke_access(
    State(services): State<Services>,
    Account(actor): Account,
    Path((library, member)): Path<(String, String)>,
) -> ApiResult<StatusCode> {
    services
        .identity
        .set_library_access(&actor, &library, &member, false)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
