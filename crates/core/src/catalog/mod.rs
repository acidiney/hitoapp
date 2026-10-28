pub(crate) mod artist_catalog;
mod artists;
mod artwork;
mod availability;
mod browse;
pub(crate) mod local_files;
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
#[cfg(test)]
#[path = "tests/album_anchor_work.rs"]
mod album_anchor_work;
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
    AlbumEditionChoice, AlbumEditionChoiceReceipt, AlbumEditionChoiceState,
    AlbumEditionTrackListAvailability, ApplyArtistIdentityToCredit, ArtistAssociation,
    ArtistAssociationChangeReceipt, ArtistAssociationChanges, ArtistAssociationEvidence,
    ArtistAssociationRetirement, ArtistAssociationUndo, ArtistCandidatePreviewAvailability,
    ArtistCandidateSearchAvailability, ArtistCorrectionRetirement, ArtistCreditRole,
    ArtistDiscographyAvailability, ArtistIdentityChangeKind, ArtistIdentityChangeReceipt,
    ArtistIdentityChangeSummary, ArtistIdentityChoiceReceipt, ArtistIdentityCreditReceipt,
    ArtistIdentityGroup, ArtistIdentityGroupBrowse, ArtistIdentityGroupChanges,
    ArtistIdentityGroupDetail, ArtistIdentityGroupLibrary, ArtistIdentityGroupMember,
    ArtistIdentityGroupName, ArtistIdentityGroupQuery, ArtistIdentitySelection,
    ArtistProfileAvailability, ArtistReviewConnection, ArtistReviewContext,
    ArtistReviewMetadataBinding, AssociateRecordingArtist, CatalogAdoption, CatalogTrackMatch,
    ChooseAlbumEdition, ChooseRecordingArtistIdentity, ClearAlbumEdition,
    EditionTrackListAvailability, MetadataProvider, MetadataSettings, PromoteArtistCorrection,
    PromotionSource, ReadAlbumEditionTrackList, ReadArtistDiscography, ReadEditionTrackList,
    ReadReleaseGroupEditions, RecordingArtistIdentityChoice, RecordingArtistIdentityState,
    ReleaseGroupEditionsAvailability, ResetRecordingArtistIdentity, SavedAlbumEdition,
    SharedArtistCorrection, SharedArtistCorrectionReceipt, SharedArtistCorrections,
    SharedCorrectionKind, SharedCorrectionRef, SplitArtistIdentityGroup, SplitDestination,
    UndoArtistIdentityChange, UndoRecordingArtistChange, UndoSharedArtistCorrection,
    UnlinkRecordingArtist, UpdateMetadataSettings,
};
pub use artists::{Artist, ArtistDetail};
pub use artwork::{
    Artwork, ArtworkAvailability, ArtworkFetch, ArtworkFetchStatus, PlaylistArtworkFetch,
    ReleaseArtworkFetch,
};
pub(crate) use availability::{SourceAccess, push_available, push_source_access};
pub use browse::Browse;
pub use releases::{AlbumCatalogAnchor, Release, ReleaseDetail, ReleaseTrack};
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
    pub(crate) local_files: local_files::LocalFiles,
}
