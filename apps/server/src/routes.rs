use crate::{
    Config,
    api::{
        self, browsing, catalog, collection, downloads, identity, jobs, player, playlist_follow,
        plugin_authorization, plugins, podcasts, progress, providers, source_bindings,
        source_choice, source_preparation, source_refresh, source_resolution, storage,
    },
    inertia::{self, props::*},
    security, telemetry,
};
use api::host_folders::{HostFolderAccess, HostFolders};
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
        PageComponent,
        SharedPageProps,
        LoginPageProps,
        LockPageProps,
        HomePageProps,
        SearchPageProps,
        CatalogPageProps,
        AlbumIndexPageProps,
        AlbumPageProps,
        AlbumDetailsPageProps,
        ArtistIndexPageProps,
        ArtistIdentityGroupPageProps,
        ArtistPageProps,
        RecordingDetailsPageProps,
        PlaylistIndexPageProps,
        PlaylistPageProps,
        AudiobookIndexPageProps,
        AudiobookPageProps,
        HistoryPageProps,
        SettingsPageProps,
        QueuePageProps,
        DiscoverPageProps,
        PodcastIndexPageProps,
        PodcastPageProps,
    ))
)]
struct Contract;

fn api_routes() -> OpenApiRouter<Services> {
    let mut router = OpenApiRouter::with_openapi(Contract::openapi())
        .routes(routes!(plugins::settings, plugins::install))
        .routes(routes!(plugins::preview))
        .routes(routes!(plugins::offer))
        .routes(routes!(plugins::withdraw))
        .routes(routes!(plugins::refresh))
        .routes(routes!(plugins::remove))
        .routes(routes!(plugins::clear_credential))
        .routes(routes!(plugin_authorization::begin))
        .routes(routes!(plugin_authorization::status))
        .routes(routes!(providers::search))
        .routes(routes!(providers::adopt))
        .routes(routes!(api::health))
        .routes(routes!(api::contract))
        .routes(routes!(identity::setup_status, identity::setup))
        .routes(routes!(identity::login))
        .routes(routes!(identity::logout))
        .routes(routes!(identity::me))
        .routes(routes!(identity::lock_status))
        .routes(routes!(identity::lock))
        .routes(routes!(identity::unlock))
        .routes(routes!(identity::record_activity))
        .routes(routes!(identity::set_pin))
        .routes(routes!(identity::remove_pin))
        .routes(routes!(identity::users, identity::create_user))
        .routes(routes!(identity::grant_access, identity::revoke_access))
        .routes(routes!(
            api::community::preferences,
            api::community::update_preferences
        ))
        .routes(routes!(
            api::community::ranking_preferences,
            api::community::update_ranking_preferences
        ))
        .routes(routes!(api::community::reset_credentials))
        .routes(routes!(api::community::successful_playback))
        .routes(routes!(podcasts::subscriptions, podcasts::subscribe))
        .routes(routes!(podcasts::episodes))
        .routes(routes!(podcasts::refresh))
        .routes(routes!(podcasts::unsubscribe))
        .routes(routes!(catalog::libraries))
        .routes(routes!(catalog::recordings))
        .routes(routes!(progress::saved_playback))
        .routes(routes!(browsing::releases))
        .routes(routes!(browsing::release))
        .routes(routes!(browsing::artists))
        .routes(routes!(browsing::artist))
        .routes(routes!(catalog::sources))
        .routes(routes!(catalog::playback_sources))
        .routes(routes!(downloads::create, downloads::status))
        .routes(routes!(
            downloads::create_playlist,
            downloads::playlist_status
        ))
        .routes(routes!(downloads::remove))
        .routes(routes!(source_bindings::review))
        .routes(routes!(source_bindings::attach))
        .routes(routes!(
            source_choice::source_choice,
            source_choice::choose_source
        ))
        .routes(routes!(source_choice::reset_source_choice))
        .routes(routes!(
            source_preparation::create,
            source_preparation::status
        ))
        .routes(routes!(source_refresh::refresh_sources))
        .routes(routes!(source_resolution::resolve_source))
        .routes(routes!(
            api::artist_catalog::settings,
            api::artist_catalog::update_settings
        ))
        .routes(routes!(api::artist_catalog::update_automation))
        .routes(routes!(api::artist_review::context))
        .routes(routes!(api::artist_review::search))
        .routes(routes!(api::artist_review::preview))
        .routes(routes!(api::artist_review::confirm))
        .routes(routes!(
            api::artist_identity::read,
            api::artist_identity::choose
        ))
        .routes(routes!(api::artist_identity::reset))
        .routes(routes!(api::artist_identity::apply_to_credit))
        .routes(routes!(api::artist_matching::run))
        .routes(routes!(api::artist_matching::run_provider_evidence))
        .routes(routes!(api::artist_matching::status))
        .routes(routes!(api::artist_matching::suggestions))
        .routes(routes!(api::artist_matching::dismiss))
        .routes(routes!(api::artist_matching::restore))
        .routes(routes!(api::artist_identity_groups::list))
        .routes(routes!(api::artist_identity_groups::read))
        .routes(routes!(api::artist_identity_groups::changes))
        .routes(routes!(api::artist_identity_groups::split))
        .routes(routes!(api::artist_identity_groups::undo))
        .routes(routes!(api::artist_corrections::list))
        .routes(routes!(api::artist_corrections::promote))
        .routes(routes!(api::artist_corrections::undo))
        .routes(routes!(api::artist_profiles::associations))
        .routes(routes!(api::artist_profiles::profile))
        .routes(routes!(api::artist_profiles::changes))
        .routes(routes!(api::artist_profiles::unlink))
        .routes(routes!(api::artist_profiles::undo))
        .routes(routes!(api::artist_discography::read))
        .routes(routes!(api::artist_discography::library))
        .routes(routes!(api::artist_similarity::read))
        .routes(routes!(api::album_editions::track_list))
        .routes(routes!(api::album_editions::track_audio))
        .routes(routes!(api::album_editions::saved_track_list))
        .routes(routes!(api::album_editions::library_level))
        .routes(routes!(api::album_editions::choose))
        .routes(routes!(api::album_editions::clear))
        .routes(routes!(api::artist_editions::list))
        .routes(routes!(api::artist_editions::track_list))
        .routes(routes!(api::lyrics::read))
        .routes(routes!(api::lyrics::providers))
        .routes(routes!(api::metadata::recording_metadata))
        .routes(routes!(api::metadata::release_metadata))
        .routes(routes!(api::artwork::artwork))
        .routes(routes!(api::artwork::artwork_availability))
        .routes(routes!(api::artwork::fetch_recording_artwork))
        .routes(routes!(api::artwork::artwork_fetch))
        .routes(routes!(api::artwork::release_artwork))
        .routes(routes!(api::collection_artwork::release_availability))
        .routes(routes!(api::collection_artwork::fetch_release))
        .routes(routes!(api::collection_artwork::release_fetch))
        .routes(routes!(api::playlist_artwork::availability))
        .routes(routes!(api::playlist_artwork::enqueue))
        .routes(routes!(api::playlist_artwork::receipt))
        .routes(routes!(api::playlist_artwork::image))
        .routes(routes!(catalog::audio, catalog::audio_head))
        .routes(routes!(jobs::scan))
        .routes(routes!(jobs::job))
        .routes(routes!(storage::directories))
        .routes(routes!(storage::browse))
        .routes(routes!(storage::folders, storage::connect))
        .routes(routes!(api::host_folders::offer))
        .routes(routes!(api::host_folders::choose))
        .routes(routes!(storage::remove))
        .routes(routes!(storage::scan))
        .routes(routes!(collection::playlists))
        .routes(routes!(collection::playlist))
        .routes(routes!(api::audiobooks::list))
        .routes(routes!(api::audiobooks::page))
        .routes(routes!(api::audiobooks::detail))
        .routes(routes!(api::audiobooks::playback))
        .routes(routes!(collection::import_provider_playlist))
        .routes(routes!(collection::refresh_imported_playlist_metadata))
        .routes(routes!(playlist_follow::follow_provider_playlist))
        .routes(routes!(playlist_follow::playlist_follow))
        .routes(routes!(playlist_follow::refresh_followed_playlist))
        .routes(routes!(collection::sync, collection::sync_events))
        .routes(routes!(collection::activity));
    router = router
        .routes(routes!(player::inspect))
        .routes(routes!(player::claim))
        .routes(routes!(player::submit))
        .routes(routes!(player::queue))
        .routes(routes!(player::audiobooks::prepare))
        .routes(routes!(player::audiobooks::cancel))
        .routes(routes!(player::observations::state))
        .routes(routes!(player::observations::position));
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
    application(services, config, None, None)
}

pub fn router_with_player(
    services: Services,
    config: Config,
    player: hito_playback::Player,
) -> Router {
    application(services, config, Some(player), None)
}

pub fn router_with_host(
    services: Services,
    config: Config,
    player: Option<hito_playback::Player>,
    folders: Arc<dyn HostFolders>,
) -> Router {
    application(services, config, player, Some(folders))
}

fn application(
    services: Services,
    config: Config,
    player: Option<hito_playback::Player>,
    folders: Option<Arc<dyn HostFolders>>,
) -> Router {
    let (api, document) = api_routes().split_for_parts();
    Router::new()
        .route("/", get(inertia::page))
        .route("/library", get(inertia::page))
        .route("/home", get(inertia::page))
        .route("/search", get(inertia::page))
        .route("/discover", get(inertia::page))
        .route("/podcasts", get(inertia::page))
        .route("/podcasts/{id}", get(inertia::page))
        .route("/albums", get(inertia::page))
        .route("/albums/{id}", get(inertia::page))
        .route("/albums/{id}/details", get(inertia::page))
        .route("/artists", get(inertia::page))
        .route("/artists/{id}", get(inertia::page))
        .route("/artist-groups/{id}", get(inertia::page))
        .route("/recordings/{id}", get(inertia::page))
        .route("/likes", get(inertia::page))
        .route("/history", get(inertia::page))
        .route("/settings", get(inertia::page))
        .route("/queue", get(inertia::page))
        .route("/playlists", get(inertia::page))
        .route("/playlists/{id}", get(inertia::page))
        .route("/audiobooks", get(inertia::page))
        .route("/audiobooks/{id}", get(inertia::page))
        .nest_service(
            "/assets",
            tower_http::services::ServeDir::new(config.web_dir.join("assets")),
        )
        .merge(api)
        .fallback(inertia::page)
        .layer(axum::extract::DefaultBodyLimit::max(262144))
        .layer(Extension(Arc::new(document)))
        .layer(Extension(config.clone()))
        .layer(Extension(player::Playback::new(player)))
        .layer(Extension(HostFolderAccess(folders)))
        .layer(middleware::from_fn_with_state(
            security::Security::new(config),
            security::browser_security,
        ))
        .layer(middleware::from_fn(telemetry::request))
        .with_state(services)
}
