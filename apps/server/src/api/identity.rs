use crate::{
    Config,
    auth::{Account, LockableAccount},
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
use hito_core::{
    AccountSummary, AuthSession, ChangePassword, Credentials, DeleteAccount, LockStatus, RemovePin,
    Services, SessionInfo, SessionsRevoked, SetPin, Unlock, User,
};
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
    headers: HeaderMap,
    JsonBody(credentials): JsonBody<Credentials>,
) -> ApiResult<Response> {
    let client = security::client_label(&headers);
    let session = services
        .identity
        .setup_with_client(credentials, Some(&client))
        .await?;
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
    headers: HeaderMap,
    JsonBody(credentials): JsonBody<Credentials>,
) -> ApiResult<Response> {
    let client = security::client_label(&headers);
    let session = services
        .identity
        .login_with_client(credentials, Some(&client))
        .await?;
    tracing::info!(event = "login_completed", "Session created");
    Ok(security::session_response(&config, session, StatusCode::OK))
}

#[utoipa::path(post, path = "/api/v1/logout", security(("session" = []), ("bearer" = [])), responses(
    (status = 204), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
))]
pub async fn logout(
    State(services): State<Services>,
    LockableAccount(_actor): LockableAccount,
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

#[utoipa::path(get, path = "/api/v1/account/lock", security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = LockStatus), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
))]
pub async fn lock_status(
    State(services): State<Services>,
    LockableAccount(actor): LockableAccount,
) -> ApiResult<Json<LockStatus>> {
    Ok(Json(services.identity.lock_status(&actor).await?))
}

#[utoipa::path(post, path = "/api/v1/account/lock", security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = LockStatus), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 422, body = ErrorResponse),
))]
pub async fn lock(
    State(services): State<Services>,
    LockableAccount(actor): LockableAccount,
) -> ApiResult<Json<LockStatus>> {
    Ok(Json(services.identity.lock(&actor).await?))
}

#[utoipa::path(post, path = "/api/v1/account/activity", security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = LockStatus), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
))]
pub async fn record_activity(
    State(services): State<Services>,
    LockableAccount(actor): LockableAccount,
) -> ApiResult<Json<LockStatus>> {
    Ok(Json(services.identity.record_activity(&actor).await?))
}

#[utoipa::path(post, path = "/api/v1/account/unlock", request_body = Unlock,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = LockStatus), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 400, body = ErrorResponse), (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse),
))]
pub async fn unlock(
    State(services): State<Services>,
    LockableAccount(actor): LockableAccount,
    JsonBody(request): JsonBody<Unlock>,
) -> ApiResult<Json<LockStatus>> {
    Ok(Json(services.identity.unlock(&actor, request).await?))
}

#[utoipa::path(put, path = "/api/v1/account/pin", request_body = SetPin,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = LockStatus), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 400, body = ErrorResponse), (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 423, body = ErrorResponse),
))]
pub async fn set_pin(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(request): JsonBody<SetPin>,
) -> ApiResult<Json<LockStatus>> {
    Ok(Json(services.identity.set_pin(&actor, request).await?))
}

#[utoipa::path(post, path = "/api/v1/account/pin/remove", request_body = RemovePin,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = LockStatus), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 400, body = ErrorResponse), (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 423, body = ErrorResponse),
))]
pub async fn remove_pin(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(request): JsonBody<RemovePin>,
) -> ApiResult<Json<LockStatus>> {
    Ok(Json(services.identity.remove_pin(&actor, request).await?))
}

#[utoipa::path(post, path = "/api/v1/account/password", request_body = ChangePassword,
    security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = SessionsRevoked), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 400, body = ErrorResponse), (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 423, body = ErrorResponse), (status = 429, body = ErrorResponse),
))]
pub async fn change_password(
    State(services): State<Services>,
    Account(actor): Account,
    JsonBody(request): JsonBody<ChangePassword>,
) -> ApiResult<Json<SessionsRevoked>> {
    Ok(Json(
        services.identity.change_password(&actor, request).await?,
    ))
}

#[utoipa::path(get, path = "/api/v1/account/sessions", security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = [SessionInfo]), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 423, body = ErrorResponse),
))]
pub async fn sessions(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<Vec<SessionInfo>>> {
    Ok(Json(services.identity.sessions(&actor).await?))
}

#[utoipa::path(delete, path = "/api/v1/account/sessions", security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = SessionsRevoked), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 423, body = ErrorResponse),
))]
pub async fn revoke_other_sessions(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<SessionsRevoked>> {
    Ok(Json(services.identity.revoke_other_sessions(&actor).await?))
}

#[utoipa::path(delete, path = "/api/v1/account/sessions/{id}", params(("id" = String, Path)),
    security(("session" = []), ("bearer" = [])), responses(
    (status = 204), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 423, body = ErrorResponse),
))]
pub async fn revoke_session(
    State(services): State<Services>,
    Account(actor): Account,
    Extension(config): Extension<Config>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    if services.identity.revoke_session(&actor, &id).await? {
        return Ok((
            StatusCode::NO_CONTENT,
            [("set-cookie", security::cookie(&config, "", 0))],
        )
            .into_response());
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[utoipa::path(get, path = "/api/v1/users", security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = [AccountSummary]), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
))]
pub async fn users(
    State(services): State<Services>,
    Account(actor): Account,
) -> ApiResult<Json<Vec<AccountSummary>>> {
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

#[utoipa::path(delete, path = "/api/v1/users/{id}", operation_id = "delete_user", params(("id" = String, Path)),
    request_body = DeleteAccount, security(("session" = []), ("bearer" = [])), responses(
    (status = 204), (status = 401, body = ErrorResponse), (status = 403, body = ErrorResponse),
    (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse),
    (status = 400, body = ErrorResponse), (status = 413, body = ErrorResponse), (status = 415, body = ErrorResponse),
    (status = 422, body = ErrorResponse), (status = 423, body = ErrorResponse),
))]
pub async fn delete_user(
    State(services): State<Services>,
    Account(actor): Account,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<DeleteAccount>,
) -> ApiResult<StatusCode> {
    services
        .identity
        .delete_account(&actor, &id, request)
        .await?;
    Ok(StatusCode::NO_CONTENT)
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
