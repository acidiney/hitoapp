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

#[path = "api/audiobook_continuation.rs"]
mod audiobook_continuation;
#[path = "api/audiobook_library.rs"]
mod audiobook_library;
#[path = "api/audiobook_pages.rs"]
mod audiobook_pages;
#[path = "api/audiobook_pages_access.rs"]
mod audiobook_pages_access;
#[path = "api/audiobooks.rs"]
mod audiobooks;
#[path = "api/audiobooks_access.rs"]
mod audiobooks_access;
#[path = "fixtures/audiobooks.rs"]
mod audiobooks_fixture;
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

#[path = "api/artist_discography.rs"]
mod artist_discography;
#[path = "api/artist_identity.rs"]
mod artist_identity;
#[path = "api/artist_identity_access.rs"]
mod artist_identity_access;
#[path = "fixtures/artist_identity.rs"]
mod artist_identity_fixture;
#[path = "api/artist_identity_inertia.rs"]
mod artist_identity_inertia;
#[path = "api/artist_identity_inertia_access.rs"]
mod artist_identity_inertia_access;
#[path = "fixtures/artist_identity_inertia.rs"]
mod artist_identity_inertia_fixture;
#[path = "api/artist_identity_inertia_lifecycle.rs"]
mod artist_identity_inertia_lifecycle;
#[path = "api/artist_identity_inertia_logs.rs"]
mod artist_identity_inertia_logs;
#[path = "api/artist_identity_inertia_query.rs"]
mod artist_identity_inertia_query;
#[path = "api/artist_identity_input.rs"]
mod artist_identity_input;
#[path = "api/artist_identity_logs.rs"]
mod artist_identity_logs;
#[path = "fixtures/artist_identity_logs.rs"]
mod artist_identity_logs_fixture;
#[path = "api/artist_identity_offline.rs"]
mod artist_identity_offline;
#[path = "api/artist_identity_receipts.rs"]
mod artist_identity_receipts;
#[path = "api/artist_review.rs"]
mod artist_review;
#[path = "api/artist_review_authority.rs"]
mod artist_review_authority;
#[path = "api/artist_review_disconnect.rs"]
mod artist_review_disconnect;
#[path = "fixtures/artist_review.rs"]
mod artist_review_fixture;
#[path = "fixtures/download_host.rs"]
mod artist_review_host;
#[path = "fixtures/artist_review_tcp.rs"]
mod artist_review_tcp;
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
