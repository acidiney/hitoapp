#[path = "api/support.rs"]
mod support;

#[path = "../../hito-core/tests/fixtures/content_service.rs"]
mod provider_fixture;

#[path = "api/identity.rs"]
mod identity;

#[path = "api/providers.rs"]
mod providers;

#[path = "api/provider_recovery.rs"]
mod provider_recovery;

#[path = "api/provider_refresh_input.rs"]
mod provider_refresh_input;

#[path = "api/provider_attachment.rs"]
mod provider_attachment;

#[path = "api/catalog.rs"]
mod catalog;

#[path = "api/browsing.rs"]
mod browsing;

#[path = "api/collection.rs"]
mod collection;

#[path = "api/collection_publication_input.rs"]
mod collection_publication_input;

#[path = "api/playlist_import.rs"]
mod playlist_import;

#[path = "api/playlist_metadata_fixture.rs"]
mod playlist_metadata_fixture;

#[path = "api/playlist_metadata_refresh.rs"]
mod playlist_metadata_refresh;

#[path = "api/playlist_metadata_refresh_guards.rs"]
mod playlist_metadata_refresh_guards;

#[path = "api/playlist_follow.rs"]
mod playlist_follow;

#[path = "api/playlist_follow_access.rs"]
mod playlist_follow_access;

#[path = "api/playlist_follow_input.rs"]
mod playlist_follow_input;

#[path = "api/playlist_follow_resume.rs"]
mod playlist_follow_resume;

#[path = "api/playlist_follow_resume_access.rs"]
mod playlist_follow_resume_access;

#[path = "api/playlist_fixture.rs"]
mod playlist_fixture;

#[path = "api/playlist_import_replay.rs"]
mod playlist_import_replay;

#[path = "api/playlist_pages.rs"]
mod playlist_pages;

#[path = "api/inertia.rs"]
mod inertia;

#[path = "api/security.rs"]
mod security;

#[path = "api/artwork.rs"]
mod artwork;

#[path = "api/metadata.rs"]
mod metadata;
#[path = "api/metadata_preferences.rs"]
mod metadata_preferences;
#[path = "api/metadata_preferences_access.rs"]
mod metadata_preferences_access;
#[path = "fixtures/metadata_preferences.rs"]
mod metadata_preferences_fixture;
#[path = "api/plugin_authorization.rs"]
mod plugin_authorization;
#[path = "api/plugins.rs"]
mod plugins;
#[path = "api/storage.rs"]
mod storage;
