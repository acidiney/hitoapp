use crate::{
    Config,
    api::{self, browsing, catalog, collection, identity, jobs, plugins, providers, storage},
    inertia::{self, props::*},
    security, telemetry,
};
use axum::{Extension, Router, middleware, routing::get};
use hito_core::Services;
use std::sync::Arc;
use utoipa::{
    OpenApi,
    openapi::{
        self,
        security::{ApiKey, ApiKeyValue, Http, HttpAuthScheme, SecurityScheme},
    },
};
use utoipa_axum::{router::OpenApiRouter, routes};

#[derive(OpenApi)]
#[openapi(
    info(title = "Hito API", version = "1.0.0"),
    components(schemas(
        SharedPageProps,
        LoginPageProps,
        CatalogPageProps,
        AlbumIndexPageProps,
        AlbumPageProps,
        AlbumDetailsPageProps,
        ArtistIndexPageProps,
        ArtistPageProps,
        RecordingDetailsPageProps,
        PlaylistIndexPageProps,
        PlaylistPageProps,
        HistoryPageProps,
        SettingsPageProps,
        DiscoverPageProps,
    ))
)]
struct Contract;

fn api_routes() -> OpenApiRouter<Services> {
    let mut router = OpenApiRouter::with_openapi(Contract::openapi())
        .routes(routes!(plugins::settings, plugins::install))
        .routes(routes!(plugins::offer))
        .routes(routes!(plugins::withdraw))
        .routes(routes!(plugins::refresh))
        .routes(routes!(plugins::remove))
        .routes(routes!(plugins::clear_credential))
        .routes(routes!(providers::search))
        .routes(routes!(providers::adopt))
        .routes(routes!(api::health))
        .routes(routes!(api::contract))
        .routes(routes!(identity::setup_status, identity::setup))
        .routes(routes!(identity::login))
        .routes(routes!(identity::logout))
        .routes(routes!(identity::me))
        .routes(routes!(identity::users, identity::create_user))
        .routes(routes!(identity::grant_access, identity::revoke_access))
        .routes(routes!(catalog::libraries))
        .routes(routes!(catalog::recordings))
        .routes(routes!(browsing::releases))
        .routes(routes!(browsing::release))
        .routes(routes!(browsing::artists))
        .routes(routes!(browsing::artist))
        .routes(routes!(catalog::sources))
        .routes(routes!(catalog::refresh_sources))
        .routes(routes!(api::metadata::recording_metadata))
        .routes(routes!(api::metadata::release_metadata))
        .routes(routes!(api::artwork::artwork))
        .routes(routes!(api::artwork::release_artwork))
        .routes(routes!(catalog::audio, catalog::audio_head))
        .routes(routes!(jobs::scan))
        .routes(routes!(jobs::job))
        .routes(routes!(storage::directories))
        .routes(routes!(storage::browse))
        .routes(routes!(storage::folders, storage::connect))
        .routes(routes!(storage::remove))
        .routes(routes!(storage::scan))
        .routes(routes!(collection::playlists))
        .routes(routes!(collection::playlist))
        .routes(routes!(collection::sync, collection::sync_events))
        .routes(routes!(collection::activity));
    if let Some(components) = router.get_openapi_mut().components.as_mut() {
        components.add_security_scheme(
            "session",
            SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new("hito_session"))),
        );
        components.add_security_scheme(
            "bearer",
            SecurityScheme::Http(Http::new(HttpAuthScheme::Bearer)),
        );
    }
    router
}

pub fn openapi() -> openapi::OpenApi {
    api_routes().into_openapi()
}

pub fn router(services: Services) -> Router {
    router_with_config(services, Config::default())
}

pub fn router_with_config(services: Services, config: Config) -> Router {
    let (api, document) = api_routes().split_for_parts();
    Router::new()
        .route("/", get(inertia::page))
        .route("/library", get(inertia::page))
        .route("/discover", get(inertia::page))
        .route("/albums", get(inertia::page))
        .route("/albums/{id}", get(inertia::page))
        .route("/albums/{id}/details", get(inertia::page))
        .route("/artists", get(inertia::page))
        .route("/artists/{id}", get(inertia::page))
        .route("/recordings/{id}", get(inertia::page))
        .route("/likes", get(inertia::page))
        .route("/history", get(inertia::page))
        .route("/settings", get(inertia::page))
        .route("/playlists", get(inertia::page))
        .route("/playlists/{id}", get(inertia::page))
        .nest_service(
            "/assets",
            tower_http::services::ServeDir::new(config.web_dir.join("assets")),
        )
        .merge(api)
        .layer(axum::extract::DefaultBodyLimit::max(262144))
        .layer(Extension(Arc::new(document)))
        .layer(Extension(config.clone()))
        .layer(middleware::from_fn_with_state(
            security::Security::new(config),
            security::browser_security,
        ))
        .layer(middleware::from_fn(telemetry::request))
        .with_state(services)
}
