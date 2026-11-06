mod artists;
mod artwork;
mod availability;
mod browse;
mod featured;
mod genres;
mod groups;
pub(crate) mod local_files;
mod provider;
pub(crate) use provider::MAX_PLAYBACK_BINDINGS;
pub(crate) use provider::publish_provider_metadata;
pub use provider::{
    AdoptProviderRecording, AttachReviewedProviderSource, PrepareSource, ProviderCandidate,
    ProviderSearch, ProviderSourceReview, ResolveSource, SourcePreparationPhase,
    SourcePreparationReceipt, SourcePreparationStatus, SourceResolution, SourceResolutionOutcome,
};
pub(crate) use provider::{AudioPermit, CatalogAudio, CatalogTrack};
pub(crate) use provider::{ExistingProviderMetadata, refresh_existing_provider_metadata};
pub(crate) mod release_availability;
pub(crate) mod releases;
pub(crate) mod search;
mod selection;
pub(crate) use selection::recordings_on;
mod source_choice;
pub(crate) use source_choice::push_source_choice;
pub use source_choice::{
    ChooseSource, ResetSourceChoice, SourceChoice, SourceChoiceEntry, SourceChoiceMode,
    SourceChoiceReceipt, SourceChoiceResetReceipt,
};
#[cfg(test)]
#[path = "tests/album_anchor_work.rs"]
mod album_anchor_work;
#[cfg(test)]
#[path = "tests/genre_work.rs"]
mod genre_work;
#[cfg(test)]
#[path = "tests/group_work.rs"]
mod group_work;
mod playback_sources;
#[cfg(test)]
#[path = "tests/release_query_work.rs"]
mod release_query_work;
mod source_preference;
pub(crate) use source_preference::{Origin, SourceRanking, provider_installations, ranking_on};
pub use source_preference::{SourcePreferenceEntry, SourcePreferences, UpdateSourcePreferences};
mod sources;
mod timed_content;
pub use timed_content::{
    LyricsLookup, ReadRecordingLyrics, RecordingLyricsProvider, RecordingLyricsProviders,
};
mod types;

use crate::{HostDeviceId, store::Store};
pub use artists::{Artist, ArtistDetail};
pub use artwork::{
    Artwork, ArtworkAvailability, ArtworkFetch, ArtworkFetchStatus, CoverArtKind, CoverArtLookup,
    PlaylistArtworkFetch, ReleaseArtworkFetch,
};
pub(crate) use artwork::{credited_artist_photo_exists, credited_artist_photos_possible};
pub(crate) use artwork::{enqueue_cover_art, queue_observed_release_covers};
pub(crate) use availability::{
    SourceAccess, push_available, push_plays_from_services, push_source_access,
};
pub use browse::Browse;
pub use featured::{FeaturedEntry, FeaturedShelf};
pub(crate) use genres::genre_identity;
pub use genres::{Genre, GenreDetail};
pub(crate) use groups::group_step;
pub use groups::{RecordingCopies, RecordingCopy, RecordingGroupProof, RecordingSplit};
pub use releases::{
    AlbumArtistInference, AlbumCatalogAnchor, Release, ReleaseDetail, ReleaseTrack,
};
use std::sync::Arc;
use tokio::sync::Semaphore;
pub use types::{Page, Recording, Search, Source};

#[derive(Clone)]
pub struct Catalog {
    pub(crate) community: crate::Community,
    pub(crate) store: Store,
    pub(crate) plugins: crate::Plugins,
    pub(crate) host: HostDeviceId,
    pub(crate) artwork_slots: Arc<Semaphore>,
    pub(crate) provider_artwork_slots: Arc<Semaphore>,
    pub(crate) resolution_callers: provider::CallerAdmission,
    pub(crate) local_files: local_files::LocalFiles,
    pub(crate) cover_art: hito_metadata_musicbrainz::CoverArtClient,
    pub(crate) artist_image: hito_metadata_musicbrainz::ArtistImageClient,
    pub(crate) featured_cache: featured::FeaturedCache,
}
