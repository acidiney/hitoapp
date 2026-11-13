#[path = "api/source_resolution_failure.rs"]
mod source_resolution_failure;

#[path = "fixtures/source_resolution_failure.rs"]
mod source_resolution_failure_fixture;

#[path = "fixtures/source_authority_snapshot.rs"]
mod source_authority_snapshot_fixture;

#[path = "api/source_authority_snapshot.rs"]
mod source_authority_snapshot;

#[path = "api/source_resolution_hydration_credentials.rs"]
mod source_resolution_hydration_credentials;

#[path = "api/source_resolution_dispatch_authority.rs"]
mod source_resolution_dispatch_authority;

#[path = "api/support.rs"]
mod support;

#[path = "fixtures/artist_review_account.rs"]
pub(crate) mod account;

#[allow(dead_code)]
#[path = "fixtures/automatic_lyric_lrclib.rs"]
mod automatic_lrclib_fixture;

#[path = "fixtures/credential_pressure.rs"]
mod credential_pressure_fixture;

#[allow(dead_code)]
#[path = "fixtures/lyric_identity_library.rs"]
mod lyric_identity_fixture;

#[path = "fixtures/source_resolution_authority.rs"]
mod source_authority_fixture;

pub(crate) use source_credential_fixture::library as source_library_fixture;

#[path = "fixtures/source_credential_lifecycle.rs"]
mod source_credential_fixture;

use hito_core::test_support::content_service as provider_fixture;

#[path = "api/featured.rs"]
mod featured;

#[path = "api/plugin_health.rs"]
mod plugin_health;

#[path = "api/lyric_service_credentials.rs"]
mod lyric_service_credentials;

#[path = "api/lyric_service_observation.rs"]
mod lyric_service_observation;

#[path = "api/lyric_service_recovery.rs"]
mod lyric_service_recovery;

#[path = "api/lyric_health_recovery.rs"]
mod lyric_health_recovery;

#[path = "api/lyric_credential_ordering.rs"]
mod lyric_credential_ordering;

#[path = "api/lyric_credential_replacement.rs"]
mod lyric_credential_replacement;

#[path = "api/source_service_observation.rs"]
mod source_service_observation;

#[path = "api/source_observation_target_reset.rs"]
mod source_observation_target_reset;

#[path = "api/source_observation_recovery.rs"]
mod source_observation_recovery;

#[path = "api/source_credential_lifecycle.rs"]
mod source_credential_lifecycle;

#[path = "api/source_credential_pressure.rs"]
mod source_credential_pressure;

#[path = "api/source_credential_pressure_inverse.rs"]
mod source_credential_pressure_inverse;

#[path = "api/account_sessions.rs"]
mod account_sessions;

#[path = "api/account_avatar.rs"]
mod account_avatar;

#[path = "api/display_name.rs"]
mod display_name;

#[path = "api/history_privacy.rs"]
mod history_privacy;
#[path = "api/recording_details_facts.rs"]
mod recording_details_facts;

#[path = "api/identity.rs"]
mod identity;

#[path = "api/pin_locks.rs"]
mod pin_locks;

#[path = "api/providers.rs"]
mod providers;

#[path = "api/provider_recovery.rs"]
mod provider_recovery;

#[path = "api/provider_refresh_input.rs"]
mod provider_refresh_input;

#[path = "api/provider_attachment.rs"]
mod provider_attachment;
#[path = "api/provider_attachment_bound.rs"]
mod provider_attachment_bound;

#[path = "api/catalog.rs"]
mod catalog;

#[path = "api/catalog_sort.rs"]
mod catalog_sort;

#[path = "api/recording_groups.rs"]
mod recording_groups;

#[path = "api/recording_lookup.rs"]
mod recording_lookup;

#[path = "api/credited_artists.rs"]
mod credited_artists;

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

#[path = "api/playlist_follow_auto_resume.rs"]
mod playlist_follow_auto_resume;

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

#[path = "api/home_search_pages.rs"]
mod home_search_pages;

#[path = "api/security.rs"]
mod security;

#[path = "api/artwork.rs"]
mod artwork;

#[path = "api/album_edition_stats.rs"]
mod album_edition_stats;
#[path = "fixtures/album_edition_stats.rs"]
mod album_edition_stats_fixture;
#[path = "api/album_editions.rs"]
mod album_editions;
#[path = "api/album_library_level.rs"]
mod album_library_level;
#[path = "api/album_tagged_edition.rs"]
mod album_tagged_edition;
#[path = "api/artist_association_changes.rs"]
mod artist_association_changes;
#[path = "api/artist_discography.rs"]
mod artist_discography;
#[path = "api/artist_discography_library.rs"]
mod artist_discography_library;
#[path = "api/artist_discography_type.rs"]
mod artist_discography_type;
#[path = "api/artist_editions.rs"]
mod artist_editions;
#[path = "api/artist_editions_cold.rs"]
mod artist_editions_cold;
#[path = "api/artist_identity.rs"]
mod artist_identity;
#[path = "api/artist_identity_access.rs"]
mod artist_identity_access;
#[path = "api/artist_identity_corrections.rs"]
mod artist_identity_corrections;
#[path = "api/artist_identity_credit.rs"]
mod artist_identity_credit;
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
#[path = "api/artist_matching.rs"]
mod artist_matching;
#[path = "api/artist_matching_provider.rs"]
mod artist_matching_provider;
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
#[path = "api/artist_review_queue.rs"]
mod artist_review_queue;
#[path = "api/artist_review_releases.rs"]
mod artist_review_releases;
#[path = "fixtures/artist_review_tcp.rs"]
mod artist_review_tcp;
#[path = "api/artist_similarity.rs"]
mod artist_similarity;
#[path = "api/artist_track_audio.rs"]
mod artist_track_audio;
#[path = "api/authorization_flow.rs"]
mod authorization_flow;
#[path = "api/catalog_album_header.rs"]
mod catalog_album_header;
#[path = "fixtures/catalog_album_header.rs"]
mod catalog_album_header_fixture;
#[path = "api/catalog_album_inertia_cold.rs"]
mod catalog_album_inertia_cold;
#[path = "api/catalog_edition_digest.rs"]
mod catalog_edition_digest;
#[path = "fixtures/credit_member_review_context_boundaries.rs"]
mod credit_member_boundary_fixture;
#[path = "api/credit_member_review_context.rs"]
mod credit_member_review_context;
#[path = "api/credit_member_review_context_boundaries.rs"]
mod credit_member_review_context_boundaries;
#[path = "fixtures/credit_member_review_credentials.rs"]
mod credit_member_review_credentials;
#[path = "fixtures/credit_member_review_credentials_provider.rs"]
mod credit_member_review_credentials_provider;
#[path = "fixtures/credit_member_review_owned.rs"]
mod credit_member_review_owned;
#[path = "api/credit_member_review_search.rs"]
mod credit_member_review_search;
#[path = "api/credit_member_review_search_credentials.rs"]
mod credit_member_review_search_credentials;
#[path = "api/credit_member_review_search_stale.rs"]
mod credit_member_review_search_stale;
#[path = "api/discover_pages.rs"]
mod discover_pages;
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
#[path = "api/plugin_authorization_callback.rs"]
mod plugin_authorization_callback;
#[path = "api/plugin_authorization_launch.rs"]
mod plugin_authorization_launch;
#[path = "api/plugin_authorization_loopback.rs"]
mod plugin_authorization_loopback;
#[path = "api/plugin_connection.rs"]
mod plugin_connection;
#[path = "api/plugins.rs"]
mod plugins;
#[path = "api/storage.rs"]
mod storage;

#[path = "api/member_search_credential_dispatch_order.rs"]
mod member_search_credential_dispatch_order;
#[path = "fixtures/member_search_credential_order.rs"]
mod member_search_credential_order_fixture;
#[path = "fixtures/member_search_credential_provider.rs"]
mod member_search_credential_provider;
#[path = "fixtures/member_search_dispatch_gate.rs"]
mod member_search_dispatch_gate;
#[path = "fixtures/member_search_order_library.rs"]
mod member_search_order_library;

#[path = "api/catalog_edition_digest_parity.rs"]
mod catalog_edition_digest_parity;
#[path = "fixtures/catalog_edition_digest_parity.rs"]
mod catalog_edition_digest_parity_fixture;

#[path = "api/credit_member_review_preview.rs"]
mod credit_member_review_preview;
#[path = "fixtures/credit_member_review_preview.rs"]
mod credit_member_review_preview_fixture;

#[path = "api/credit_member_review_preview_security.rs"]
mod credit_member_review_preview_security;
#[path = "fixtures/credit_member_review_preview_security.rs"]
mod credit_member_review_preview_security_fixture;

#[path = "api/credit_member_review_confirm.rs"]
mod credit_member_review_confirm;

#[path = "api/credit_member_review_main_target.rs"]
mod credit_member_review_main_target;

#[path = "api/credit_member_identity_choice.rs"]
mod credit_member_identity_choice;
#[path = "fixtures/credit_member_identity_choice.rs"]
mod credit_member_identity_choice_fixture;

#[path = "api/artist_identity_read_fence.rs"]
mod artist_identity_read_fence;
#[path = "fixtures/artist_identity_read_fence.rs"]
mod artist_identity_read_fence_fixture;

#[path = "api/credit_member_identity_reset.rs"]
mod credit_member_identity_reset;

#[path = "fixtures/credit_member_identity_reset.rs"]
mod credit_member_identity_reset_fixture;

#[path = "api/credit_member_identity_reset_authority.rs"]
mod credit_member_identity_reset_authority;
#[path = "fixtures/credit_member_identity_reset_authority.rs"]
mod credit_member_identity_reset_authority_fixture;

#[path = "api/credit_member_identity_reset_validation.rs"]
mod credit_member_identity_reset_validation;
