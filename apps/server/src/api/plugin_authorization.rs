use super::plugins::PluginRevision;
use crate::{
    Config,
    auth::{Account, OptionalAccount},
    error::{ApiResult, ErrorResponse},
    extract::JsonBody,
};
use axum::{
    Extension, Json,
    extract::{Path, RawQuery, State},
    http::{HeaderMap, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use hito_core::{
    PluginAuthorization, PluginAuthorizationCallback, PluginAuthorizationOutcome,
    PluginAuthorizationRedirect, PluginAuthorizationStatus, Services,
};

#[utoipa::path(post, path = "/api/v1/plugins/{id}/authorization", operation_id = "begin_plugin_authorization", params(("id" = String, Path)), request_body = PluginRevision, security(("session" = []), ("bearer" = [])), responses(
    (status = 201, body = PluginAuthorization), (status = 401, body = ErrorResponse), (status = 404, body = ErrorResponse), (status = 409, body = ErrorResponse), (status = 422, body = ErrorResponse),
))]
pub async fn begin(
    State(services): State<Services>,
    Account(actor): Account,
    Extension(config): Extension<Config>,
    Path(id): Path<String>,
    JsonBody(request): JsonBody<PluginRevision>,
) -> ApiResult<(
    StatusCode,
    [(&'static str, &'static str); 1],
    Json<PluginAuthorization>,
)> {
    let origin_uri: Uri = config.origin.parse().map_err(|_| invalid_origin())?;
    if origin_uri.scheme().is_none()
        || origin_uri.authority().is_none()
        || origin_uri.path() != "/"
        || origin_uri.query().is_some()
    {
        return Err(invalid_origin().into());
    }
    let origin = config.origin.strip_suffix('/').unwrap_or(&config.origin);
    let redirect = PluginAuthorizationRedirect::new(&format!(
        "{origin}/api/v1/plugin-authorization/callback"
    ))?;
    let attempt = services
        .plugins
        .begin_authorization(&actor, &id, request.revision, &redirect)
        .await?;
    Ok((
        StatusCode::CREATED,
        [("cache-control", "no-store")],
        Json(attempt),
    ))
}

#[utoipa::path(get, path = "/api/v1/plugins/{id}/authorization/{attempt}", operation_id = "plugin_authorization_status", params(("id" = String, Path), ("attempt" = String, Path)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, body = PluginAuthorizationStatus), (status = 401, body = ErrorResponse), (status = 404, body = ErrorResponse),
))]
pub async fn status(
    State(services): State<Services>,
    Account(actor): Account,
    Path((id, attempt)): Path<(String, String)>,
) -> ApiResult<(
    [(&'static str, &'static str); 1],
    Json<PluginAuthorizationStatus>,
)> {
    let status = services
        .plugins
        .authorization_status(&actor, &id, &attempt)
        .await?;
    Ok(([("cache-control", "no-store")], Json(status)))
}

fn invalid_origin() -> hito_core::Error {
    hito_core::Error::Invalid("Configure HITO_PUBLIC_ORIGIN as an HTTPS origin or a loopback IP over HTTP before connecting this provider".into())
}

#[utoipa::path(get, path = "/api/v1/plugin-authorization/callback", operation_id = "plugin_authorization_callback", params(("state" = Option<String>, Query), ("code" = Option<String>, Query), ("error" = Option<String>, Query)), security(("session" = []), ("bearer" = [])), responses(
    (status = 200, content_type = "text/html"), (status = 400, content_type = "text/html"), (status = 401, content_type = "text/html"), (status = 409, content_type = "text/html"), (status = 502, content_type = "text/html"), (status = 503, content_type = "text/html"),
))]
pub async fn callback(
    State(services): State<Services>,
    OptionalAccount(actor): OptionalAccount,
    Extension(config): Extension<Config>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    let Some(actor) = actor else {
        let cross_site = headers
            .get("sec-fetch-site")
            .is_some_and(|value| value == "cross-site" || value == "same-site");
        let retried = headers
            .get_all("cookie")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(';'))
            .any(|value| value.trim() == "hito_callback_retry=1");
        tracing::info!(
            event = "authorization_callback_unauthenticated",
            same_origin_retry_offered = cross_site && !retried,
            "Provider authorization callback arrived without a session"
        );
        if cross_site && !retried {
            let secure = if config.origin.starts_with("https://") {
                "; Secure"
            } else {
                ""
            };
            let mut response = page(StatusCode::OK, RELOAD_PAGE.to_owned());
            response.headers_mut().insert(
                "set-cookie",
                format!("hito_callback_retry=1; Path=/api/v1/plugin-authorization/callback; Max-Age=30; HttpOnly; SameSite=Lax{secure}")
                    .parse()
                    .expect("static cookie"),
            );
            return response;
        }
        return outcome_page(
            StatusCode::UNAUTHORIZED,
            "Sign in to Hito",
            "Sign in to Hito in this browser, then start the provider connection again from Settings.",
        );
    };
    let Some(callback) = PluginAuthorizationCallback::parse(query.as_deref()) else {
        return outcome_response(PluginAuthorizationOutcome::Invalid);
    };
    if let Err(error) = actor.require_unlocked() {
        return match error {
            hito_core::Error::Locked => outcome_page(
                StatusCode::LOCKED,
                "Hito is locked",
                "Unlock Hito, then start the provider connection again from Settings. Your attempt was not used.",
            ),
            error => crate::error::ApiError(error).into_response(),
        };
    }
    match services
        .plugins
        .complete_authorization(&actor, callback)
        .await
    {
        Ok(outcome) => outcome_response(outcome),
        Err(hito_core::Error::ProviderBusy(_)) => outcome_page(
            StatusCode::SERVICE_UNAVAILABLE,
            "Hito is busy",
            "Provider connections are busy. Your attempt was not used. Return to Settings and try again in a moment.",
        ),
        Err(error) => crate::error::ApiError(error).into_response(),
    }
}

const RELOAD_PAGE: &str = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><meta http-equiv=\"refresh\" content=\"1\"><title>Hito</title></head><body><p>Finishing your provider connection...</p></body></html>";

fn outcome_response(outcome: PluginAuthorizationOutcome) -> Response {
    use PluginAuthorizationOutcome as Outcome;
    let (status, title, message) = match outcome {
        Outcome::Connected => (
            StatusCode::OK,
            "Provider authorized",
            "The provider approved Hito. You can close this tab and return to Hito.",
        ),
        Outcome::Denied => (
            StatusCode::OK,
            "Connection declined",
            "The provider did not grant access. Nothing was connected. You can start again from Settings.",
        ),
        Outcome::ProviderError => (
            StatusCode::BAD_REQUEST,
            "The provider reported a problem",
            "The provider could not complete the connection. Start again from Settings.",
        ),
        Outcome::ExchangeFailed => (
            StatusCode::BAD_GATEWAY,
            "Hito could not finish the connection",
            "The provider did not accept the approval. Start again from Settings.",
        ),
        Outcome::Expired => (
            StatusCode::BAD_REQUEST,
            "This connection attempt expired",
            "Start again from Settings.",
        ),
        Outcome::Changed => (
            StatusCode::CONFLICT,
            "The connection changed",
            "This plugin or your sign-in changed while you were approving. Reload Settings and start again.",
        ),
        Outcome::Used => (
            StatusCode::CONFLICT,
            "This connection attempt was already used",
            "Start again from Settings if you still need to connect.",
        ),
        Outcome::Invalid => (
            StatusCode::BAD_REQUEST,
            "This connection link is not valid",
            "Open Settings in the browser where you started the connection, then try again.",
        ),
    };
    outcome_page(status, title, message)
}

fn outcome_page(status: StatusCode, title: &str, message: &str) -> Response {
    page(
        status,
        format!(
            "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Hito</title></head><body><main><h1>{title}</h1><p>{message}</p><p><a href=\"/\">Return to Hito</a></p></main></body></html>"
        ),
    )
}

fn page(status: StatusCode, body: String) -> Response {
    (
        status,
        [
            ("content-type", "text/html; charset=utf-8"),
            ("cache-control", "no-store"),
            ("referrer-policy", "no-referrer"),
        ],
        body,
    )
        .into_response()
}
