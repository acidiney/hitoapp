pub(crate) mod artist_catalog;
mod artists;
mod artwork;
pub(crate) mod audio;
mod availability;
mod browse;
mod candidates;
pub use candidates::PlaybackCandidates;
mod media;
mod playback;
pub use playback::PlaybackSource;
mod preferred;
mod provider;
pub(crate) use provider::MAX_PLAYBACK_BINDINGS;
pub(crate) use provider::publish_provider_metadata;
pub use provider::{
    AdoptProviderRecording, AttachReviewedProviderSource, PrepareSource, ProviderCandidate,
    ProviderSearch, ProviderSourceReview, ResolveSource, SourcePreparationPhase,
    SourcePreparationReceipt, SourcePreparationStatus, SourceResolution, SourceResolutionOutcome,
};
pub(crate) use provider::{ExistingProviderMetadata, refresh_existing_provider_metadata};
mod releases;
mod search;
mod selection;
pub(crate) use selection::recordings_on;
mod source_choice;
pub(crate) use source_choice::push_source_choice;
pub use source_choice::{
    ChooseSource, ResetSourceChoice, SourceChoice, SourceChoiceEntry, SourceChoiceMode,
    SourceChoiceReceipt, SourceChoiceResetReceipt,
};
mod playback_sources;
#[cfg(test)]
#[path = "tests/release_query_work.rs"]
mod release_query_work;
mod sources;
mod timed_content;
pub use timed_content::{
    LyricsLookup, ReadRecordingLyrics, RecordingLyricsProvider, RecordingLyricsProviders,
};
mod types;

use crate::{HostDeviceId, store::Store};
pub use artist_catalog::{
    ArtistAssociation, ArtistAssociationEvidence, ArtistCandidatePreviewAvailability,
    ArtistCandidateSearchAvailability, ArtistCreditRole, ArtistDiscographyAvailability,
    ArtistIdentityChoiceReceipt, ArtistIdentityGroup, ArtistIdentityGroupBrowse,
    ArtistIdentityGroupDetail, ArtistIdentityGroupLibrary, ArtistIdentityGroupMember,
    ArtistIdentityGroupName, ArtistIdentityGroupQuery, ArtistIdentitySelection,
    ArtistProfileAvailability, ArtistReviewConnection, ArtistReviewContext,
    ArtistReviewMetadataBinding, AssociateRecordingArtist, ChooseRecordingArtistIdentity,
    EditionTrackListAvailability, MetadataProvider, MetadataSettings, ReadArtistDiscography,
    ReadEditionTrackList, ReadReleaseGroupEditions, RecordingArtistIdentityChoice,
    RecordingArtistIdentityState, ReleaseGroupEditionsAvailability, ResetRecordingArtistIdentity,
    UpdateMetadataSettings,
};
pub use artists::{Artist, ArtistDetail};
pub use artwork::{
    Artwork, ArtworkAvailability, ArtworkFetch, ArtworkFetchStatus, PlaylistArtworkFetch,
    ReleaseArtworkFetch,
};
pub use audio::{AudioContent, AudioRequest, AudioStream, ByteRange, OpenedAudio};
pub(crate) use availability::{SourceAccess, push_available, push_source_access};
pub use browse::Browse;
pub use media::AudioFile;
pub use releases::{Release, ReleaseDetail, ReleaseTrack};
use std::sync::Arc;
use tokio::sync::Semaphore;
pub use types::{Page, Recording, Search, Source};

#[derive(Clone)]
pub struct Catalog {
    pub(crate) community: crate::Community,
    pub(crate) store: Store,
    pub(crate) plugins: crate::Plugins,
    pub(crate) artist_catalog: artist_catalog::Host,
    pub(crate) host: HostDeviceId,
    pub(crate) artwork_slots: Arc<Semaphore>,
    pub(crate) provider_artwork_slots: Arc<Semaphore>,
    pub(crate) resolution_callers: provider::CallerAdmission,
    pub(crate) directories: crate::library_storage::Directories,
    pub(crate) relay: audio::AudioRelay,
}
