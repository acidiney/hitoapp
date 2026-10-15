mod html;
mod page;
pub mod props;
mod protocol;

use crate::{Config, auth::OptionalAccount, error::ApiResult, extract::QueryParams};
use axum::{
    Extension, Json,
    extract::{OriginalUri, State},
    http::HeaderMap,
    response::{Html, IntoResponse, Response},
};
use hito_core::{Search, Services};
use page::Screen;
use props::{LoginPageProps, PageProps};
use protocol::Protocol;
use serde_json::json;

#[derive(serde::Deserialize)]
pub(super) struct ArtistPagination {
    recording_offset: Option<i64>,
}

pub async fn page(
    State(services): State<Services>,
    Extension(config): Extension<Config>,
    OptionalAccount(actor): OptionalAccount,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    QueryParams(search): QueryParams<Search>,
    QueryParams(pagination): QueryParams<ArtistPagination>,
) -> ApiResult<Response> {
    if let Some(response) =
        Protocol::version_conflict(&headers, &config.asset_version, &uri.to_string())
    {
        return Ok(response);
    }
    let screen = Screen::from_path(uri.path());
    let component = if actor.is_some() {
        screen.component()
    } else {
        "LoginPage"
    };
    let protocol = Protocol::new(&headers, component);
    let clear_history = actor.is_none();
    let props = match actor {
        Some(actor) => {
            screen
                .props(&services, &actor, search, pagination, &protocol)
                .await?
        }
        None => {
            let needs_setup = if protocol.requested("needs_setup") {
                Some(services.identity.needs_setup().await?)
            } else {
                None
            };
            PageProps::Login(LoginPageProps {
                errors: Default::default(),
                needs_setup,
            })
        }
    };
    tracing::debug!(
        event = "inertia_page",
        component = props.component(),
        partial = headers.contains_key("x-inertia-partial-component"),
        "Page assembled"
    );
    let page = json!({"component":props.component(), "props":props, "url":uri.to_string(), "version":config.asset_version, "encryptHistory":true, "clearHistory":clear_history});
    if protocol.inertia {
        Ok(([("x-inertia", "true"), ("vary", "X-Inertia")], Json(page)).into_response())
    } else {
        Ok(([("vary", "X-Inertia")], Html(html::render(&page))).into_response())
    }
}
