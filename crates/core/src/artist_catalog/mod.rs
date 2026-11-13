mod album_audio;
mod album_edition_choice;
mod album_editions;
mod album_library_level;
mod album_tagged_edition;
mod association_changes;
mod associations;
mod authority;
pub(crate) mod automation;
mod candidate_releases;
mod confirmation;
mod continuation;
mod credit_target;
mod discography;
mod discovery;
mod edition_stats;
mod editions;
pub(crate) mod erasure;
mod genre_lookups;
pub(crate) mod identity;
mod library_availability;
mod matching;
mod policy;
mod preview;
mod profile;
mod provider;
mod radio;
pub(crate) mod reconcile;
mod release_group;
mod review;
mod shared_corrections;
mod similar_artists;
mod track_links;
mod types;
pub use identity::*;
pub use library_availability::{
    DiscographyLibrary, LibraryAlbum, LibraryReleaseGroup, ReadDiscographyLibrary,
};
pub use matching::*;
pub(crate) use matching::{Title, tolerance_ms};

pub use album_audio::{AlbumTrackAudio, FindAlbumTrackAudio, FindArtistTrackAudio};
pub use album_edition_choice::{
    AlbumEditionChoice, AlbumEditionChoiceReceipt, AlbumEditionChoiceState, ChooseAlbumEdition,
    ClearAlbumEdition, SavedAlbumEdition,
};
pub use album_editions::{
    AlbumEditionTrackListAvailability, CatalogAdoption, CatalogTrackMatch,
    ReadAlbumEditionTrackList,
};
pub use album_library_level::{AlbumLibraryLevel, LibraryLevel, ReadAlbumLibraryLevel};
pub use album_tagged_edition::TaggedAlbumEdition;
pub use association_changes::{
    ArtistAssociationChangeReceipt, ArtistAssociationChanges, ArtistAssociationRetirement,
    ArtistAssociationUndo, ArtistCorrectionRetirement, UndoRecordingArtistChange,
    UnlinkRecordingArtist,
};
pub(crate) use associations::LINKED;
pub use edition_stats::EditionTrackStats;
pub use radio::ArtistRadio;
pub use release_group::{ReadReleaseGroup, ReleaseGroupAvailability};
pub use shared_corrections::{
    PromoteArtistCorrection, PromotionSource, SharedArtistCorrection, SharedArtistCorrectionQuery,
    SharedArtistCorrectionReceipt, SharedArtistCorrections, SharedCorrectionKind,
    SharedCorrectionRef, UndoSharedArtistCorrection,
};
pub use similar_artists::{ReadSimilarArtists, SimilarArtistView, SimilarArtistsAvailability};
pub use types::{
    ArtistAssociation, ArtistAssociationEvidence, ArtistCandidatePreviewAvailability,
    ArtistCandidateReleasesAvailability, ArtistCandidateSearchAvailability, ArtistCreditRole,
    ArtistDiscographyAvailability, ArtistMatchingState, ArtistMatchingStatus,
    ArtistProfileAvailability, ArtistReviewConnection, ArtistReviewContext,
    ArtistReviewMetadataBinding, AssociateRecordingArtist, CoverageEvidence, CreditEvidence,
    CreditEvidenceKind, CreditMemberCandidatePreviewAvailability,
    CreditMemberCandidateSearchAvailability, CreditMemberReviewContext,
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
    release_group_slots: Arc<Semaphore>,
    release_group_accounts: Accounts,
    background_slots: Arc<Semaphore>,
    background_accounts: Accounts,
    continuations: continuation::Key,
}

impl ArtistCatalog {
    pub(crate) fn new(
        catalog: Catalog,
        store: Store,
        plugins: Plugins,
        host: HostDeviceId,
        builtin: MusicBrainzClient,
    ) -> Result<Self> {
        Ok(Self {
            catalog,
            store,
            plugins,
            host,
            builtin,
            slots: Arc::new(Semaphore::new(4)),
            accounts: Accounts::new(1),
            release_group_slots: Arc::new(Semaphore::new(2)),
            release_group_accounts: Accounts::new(1),
            background_slots: Arc::new(Semaphore::new(1)),
            background_accounts: Accounts::new(1),
            continuations: continuation::Key::new(),
        })
    }

    fn admit_matching(
        &self,
        actor: &crate::Actor,
    ) -> Result<(OwnedSemaphorePermit, OwnedSemaphorePermit)> {
        if !actor.is_unattended() {
            return self.admit(actor.id());
        }
        let global = self
            .background_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                Error::Conflict("Background artist matching is busy. Try again.".into())
            })?;
        Ok((global, self.background_accounts.enter(actor.id())?))
    }

    fn admit(&self, account: &str) -> Result<(OwnedSemaphorePermit, OwnedSemaphorePermit)> {
        let global =
            self.slots.clone().try_acquire_owned().map_err(|_| {
                Error::Conflict("Artist profile requests are busy. Try again.".into())
            })?;
        let account = self.accounts.enter(account)?;
        Ok((global, account))
    }

    fn admit_release_group(
        &self,
        account: &str,
    ) -> Result<(OwnedSemaphorePermit, OwnedSemaphorePermit)> {
        let global = self
            .release_group_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Conflict("Album metadata requests are busy. Try again.".into()))?;
        let account = self.release_group_accounts.enter(account)?;
        Ok((global, account))
    }
}
