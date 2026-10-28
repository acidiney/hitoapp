mod html;
mod pages;
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
use pages::Screen;
use props::{LockPageProps, LoginPageProps, PageComponent, PageProps};
use protocol::Protocol;
use serde_json::json;

#[derive(serde::Deserialize)]
pub(super) struct ArtistPagination {
    group_offset: Option<i64>,
    library_offset: Option<i64>,
    recording_offset: Option<i64>,
    release_offset: Option<i64>,
    cursor: Option<String>,
}

pub async fn page(
    State(services): State<Services>,
    (Extension(config), Extension(playback)): (
        Extension<Config>,
        Extension<crate::api::player::Playback>,
    ),
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
    let locked = actor.as_ref().is_some_and(|actor| actor.is_locked());
    let component = if locked {
        PageComponent::LockPage
    } else if actor.is_some() {
        screen.component()
    } else {
        PageComponent::LoginPage
    };
    let protocol = Protocol::new(&headers, component);
    let clear_history = actor.is_none() || locked;
    let props = match actor {
        Some(actor) if actor.is_locked() => PageProps::Lock(LockPageProps {
            errors: Default::default(),
            locked: true,
            username: actor.user().username.clone(),
            account_id: actor.user().id.clone(),
        }),
        Some(actor) => {
            screen
                .props(
                    &services,
                    &actor,
                    search,
                    pagination,
                    &protocol,
                    playback.native_enabled(),
                )
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
        component = %props.component(),
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
