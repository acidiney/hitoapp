#[cfg(test)]
pub(crate) mod work_meter;

#[cfg(feature = "test-support")]
pub mod album_edition_service;
#[cfg(feature = "test-support")]
pub mod artist_catalog_discography_service;
#[cfg(feature = "test-support")]
pub mod artist_catalog_discovery_service;
#[cfg(feature = "test-support")]
pub mod artist_catalog_editions_service;
#[cfg(feature = "test-support")]
pub mod artist_catalog_profiles_service;
#[cfg(feature = "test-support")]
pub mod artist_catalog_similarity_service;
#[cfg(feature = "test-support")]
pub mod artist_identity_selection_catalog;
#[cfg(feature = "test-support")]
pub mod artist_images;
#[cfg(feature = "test-support")]
pub mod audiobook_continue_library;
#[cfg(feature = "test-support")]
pub mod collection_artwork;
#[cfg(feature = "test-support")]
pub mod community_ranking_library;
#[cfg(feature = "test-support")]
pub mod community_rankings;
#[cfg(feature = "test-support")]
pub mod community_recovery;
#[cfg(feature = "test-support")]
pub mod community_service;
#[cfg(feature = "test-support")]
pub mod content_service;
#[cfg(feature = "test-support")]
pub mod correction_erasure;
#[cfg(feature = "test-support")]
pub mod cover_art;
#[cfg(feature = "test-support")]
pub mod covers;
#[cfg(feature = "test-support")]
pub mod featured_service;
#[cfg(feature = "test-support")]
pub mod hang_guard;
#[cfg(feature = "test-support")]
pub mod media_tool;
#[cfg(feature = "test-support")]
pub mod metadata_tag_logs;
#[cfg(feature = "test-support")]
pub mod musicbrainz;
#[cfg(feature = "test-support")]
pub mod observed;
#[cfg(feature = "test-support")]
pub mod playback;
#[cfg(feature = "test-support")]
pub mod playlist_service;
#[cfg(feature = "test-support")]
pub mod plugin_service;
#[cfg(feature = "test-support")]
pub mod podcast_audio_service;
#[cfg(feature = "test-support")]
pub mod provider_audio;
#[cfg(feature = "test-support")]
pub mod support;

#[cfg(feature = "test-support")]
pub fn migrations() -> &'static sqlx::migrate::Migrator {
    &crate::store::MIGRATOR
}
