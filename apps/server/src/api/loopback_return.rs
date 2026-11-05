use super::{
    callback_page::{CallbackPage, Tone},
    plugin_authorization::outcome_page,
};
use axum::{
    Router,
    extract::{RawQuery, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use hito_core::{PluginAuthorizationCallback, PluginAuthorizationOutcome, Services};

pub const CALLBACK_PATH: &str = "/api/v1/plugin-authorization/callback";

pub fn router() -> Router<Services> {
    Router::new()
        .route(CALLBACK_PATH, get(callback))
        .fallback(unknown)
}

async fn unknown() -> Response {
    (
        StatusCode::NOT_FOUND,
        [
            ("cache-control", "no-store"),
            ("referrer-policy", "no-referrer"),
        ],
    )
        .into_response()
}

async fn callback(State(services): State<Services>, RawQuery(query): RawQuery) -> Response {
    let Some(callback) = PluginAuthorizationCallback::parse(query.as_deref()) else {
        return invalid();
    };
    match services.plugins.complete_loopback_authorization(callback).await {
        Ok(PluginAuthorizationOutcome::Invalid) => invalid(),
        Ok(outcome) => outcome_page(outcome).without_return_link().into_response(),
        Err(hito_core::Error::Locked) => CallbackPage::outcome(
            StatusCode::LOCKED,
            Tone::Locked,
            "Hito is locked",
            "Unlock Hito, then start the provider connection again from Settings. Your attempt was not used.",
        )
        .without_return_link()
        .into_response(),
        Err(hito_core::Error::ProviderBusy(_)) => CallbackPage::outcome(
            StatusCode::SERVICE_UNAVAILABLE,
            Tone::Notice,
            "Hito is busy",
            "Provider connections are busy. Your attempt was not used. Return to Settings and try again in a moment.",
        )
        .without_return_link()
        .into_response(),
        Err(error) => crate::error::ApiError(error).into_response(),
    }
}

fn invalid() -> Response {
    CallbackPage::outcome(
        StatusCode::BAD_REQUEST,
        Tone::Failure,
        "This connection link is not valid",
        "Open Hito and start the connection again from Settings.",
    )
    .without_return_link()
    .into_response()
}
