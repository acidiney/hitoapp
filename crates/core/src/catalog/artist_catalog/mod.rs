mod album_edition_choice;
mod album_editions;
mod association_changes;
mod associations;
mod authority;
mod confirmation;
mod continuation;
mod discography;
mod discovery;
mod editions;
pub(crate) mod identity;
mod policy;
mod preview;
mod profile;
mod provider;
mod review;
mod types;
pub use identity::*;

pub use album_edition_choice::{
    AlbumEditionChoice, AlbumEditionChoiceReceipt, AlbumEditionChoiceState, ChooseAlbumEdition,
    ClearAlbumEdition, SavedAlbumEdition,
};
pub use album_editions::{
    AlbumEditionTrackListAvailability, CatalogAdoption, CatalogTrackMatch,
    ReadAlbumEditionTrackList,
};
pub use association_changes::{
    ArtistAssociationChangeReceipt, ArtistAssociationChanges, ArtistAssociationUndo,
    UndoRecordingArtistChange, UnlinkRecordingArtist,
};
pub(crate) use associations::LINKED;
pub use types::{
    ArtistAssociation, ArtistAssociationEvidence, ArtistCandidatePreviewAvailability,
    ArtistCandidateSearchAvailability, ArtistCreditRole, ArtistDiscographyAvailability,
    ArtistProfileAvailability, ArtistReviewConnection, ArtistReviewContext,
    ArtistReviewMetadataBinding, AssociateRecordingArtist, EditionTrackListAvailability,
    MetadataProvider, MetadataSettings, ReadArtistDiscography, ReadEditionTrackList,
    ReadReleaseGroupEditions, ReleaseGroupEditionsAvailability, UpdateMetadataSettings,
};

use crate::{Error, Result, admission::Accounts};
use hito_metadata_musicbrainz::MusicBrainzClient;
use std::{sync::Arc, time::Duration};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const REQUEST_BUDGET: Duration = Duration::from_secs(15);
const MAX_ASSOCIATIONS: i64 = 32;

#[derive(Clone)]
pub(crate) struct Host {
    builtin: MusicBrainzClient,
    slots: Arc<Semaphore>,
    accounts: Accounts,
    continuations: continuation::Key,
}

impl Host {
    pub(crate) fn new() -> Result<Self> {
        Ok(Self {
            builtin: MusicBrainzClient::new().map_err(|_| {
                Error::Internal("The built-in metadata client could not be initialized".into())
            })?,
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
