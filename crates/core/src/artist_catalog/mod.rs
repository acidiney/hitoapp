mod album_audio;
mod album_edition_choice;
mod album_editions;
mod album_library_level;
mod association_changes;
mod associations;
mod authority;
pub(crate) mod automation;
mod confirmation;
mod continuation;
mod discography;
mod discovery;
mod editions;
pub(crate) mod erasure;
pub(crate) mod identity;
mod library_availability;
mod matching;
mod policy;
mod preview;
mod profile;
mod provider;
pub(crate) mod reconcile;
mod review;
mod shared_corrections;
mod similar_artists;
mod types;
pub use identity::*;
pub use library_availability::{
    DiscographyLibrary, LibraryAlbum, LibraryReleaseGroup, ReadDiscographyLibrary,
};
pub use matching::*;

pub use album_audio::{AlbumTrackAudio, FindAlbumTrackAudio};
pub use album_edition_choice::{
    AlbumEditionChoice, AlbumEditionChoiceReceipt, AlbumEditionChoiceState, ChooseAlbumEdition,
    ClearAlbumEdition, SavedAlbumEdition,
};
pub use album_editions::{
    AlbumEditionTrackListAvailability, CatalogAdoption, CatalogTrackMatch,
    ReadAlbumEditionTrackList,
};
pub use album_library_level::{AlbumLibraryLevel, LibraryLevel, ReadAlbumLibraryLevel};
pub use association_changes::{
    ArtistAssociationChangeReceipt, ArtistAssociationChanges, ArtistAssociationRetirement,
    ArtistAssociationUndo, ArtistCorrectionRetirement, UndoRecordingArtistChange,
    UnlinkRecordingArtist,
};
pub(crate) use associations::LINKED;
pub use shared_corrections::{
    PromoteArtistCorrection, PromotionSource, SharedArtistCorrection, SharedArtistCorrectionQuery,
    SharedArtistCorrectionReceipt, SharedArtistCorrections, SharedCorrectionKind,
    SharedCorrectionRef, UndoSharedArtistCorrection,
};
pub use similar_artists::{ReadSimilarArtists, SimilarArtistView, SimilarArtistsAvailability};
pub use types::{
    ArtistAssociation, ArtistAssociationEvidence, ArtistCandidatePreviewAvailability,
    ArtistCandidateSearchAvailability, ArtistCreditRole, ArtistDiscographyAvailability,
    ArtistMatchingState, ArtistMatchingStatus, ArtistProfileAvailability, ArtistReviewConnection,
    ArtistReviewContext, ArtistReviewMetadataBinding, AssociateRecordingArtist,
    EditionTrackListAvailability, MetadataProvider, MetadataSettings, ReadArtistDiscography,
    ReadEditionTrackList, ReadReleaseGroupEditions, ReleaseGroupEditionsAvailability,
    UpdateAutomaticLookups, UpdateMetadataSettings,
};

use crate::{Catalog, Error, HostDeviceId, Plugins, Result, admission::Accounts, store::Store};
use hito_metadata_musicbrainz::MusicBrainzClient;
use std::{sync::Arc, time::Duration};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const REQUEST_BUDGET: Duration = Duration::from_secs(15);
const MAX_ASSOCIATIONS: i64 = 32;
const MAX_RETAINED_ASSOCIATIONS: i64 = 128;

#[derive(Clone)]
pub struct ArtistCatalog {
    pub(crate) catalog: Catalog,
    pub(crate) store: Store,
    pub(crate) plugins: Plugins,
    pub(crate) host: HostDeviceId,
    builtin: MusicBrainzClient,
    slots: Arc<Semaphore>,
    accounts: Accounts,
    continuations: continuation::Key,
}

impl ArtistCatalog {
    pub(crate) fn new(
        catalog: Catalog,
        store: Store,
        plugins: Plugins,
        host: HostDeviceId,
        #[cfg(any(test, feature = "test-support"))] musicbrainz_loopback_endpoint: Option<url::Url>,
    ) -> Result<Self> {
        #[cfg(any(test, feature = "test-support"))]
        let builtin = match musicbrainz_loopback_endpoint {
            Some(endpoint) => MusicBrainzClient::with_loopback_endpoint(endpoint),
            None => MusicBrainzClient::new(),
        };
        #[cfg(not(any(test, feature = "test-support")))]
        let builtin = MusicBrainzClient::new();
        let builtin = builtin.map_err(|_| {
            Error::Internal("The built-in metadata client could not be initialized".into())
        })?;
        Ok(Self {
            catalog,
            store,
            plugins,
            host,
            builtin,
            slots: Arc::new(Semaphore::new(4)),
            accounts: Accounts::new(1),
            continuations: continuation::Key::new(),
        })
    }

    fn admit(&self, account: &str) -> Result<(OwnedSemaphorePermit, OwnedSemaphorePermit)> {
        let global =
            self.slots.clone().try_acquire_owned().map_err(|_| {
                Error::Conflict("Artist profile requests are busy. Try again.".into())
            })?;
        let account = self.accounts.enter(account)?;
        Ok((global, account))
    }
}
