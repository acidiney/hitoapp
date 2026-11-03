use super::{
    ArtistCatalog, MetadataProvider, REQUEST_BUDGET, SavedAlbumEdition,
    album_edition_choice::stored, matching::LOOKUP_DEADLINE, policy,
};
use crate::{
    Actor, Error, Result,
    catalog::releases::{release_contains, releases_by_ids_on},
    metadata::tagged_release,
};
use hito_metadata_musicbrainz::Error as MusicBrainzError;
use serde::Serialize;
use std::{future::Future, time::Instant};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TaggedAlbumEdition {
    Found { edition: SavedAlbumEdition },
    None,
}

type Outcome = (TaggedAlbumEdition, &'static str);

fn nothing(reason: &'static str) -> Result<Outcome> {
    Ok((TaggedAlbumEdition::None, reason))
}

impl ArtistCatalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "tagged_album_edition", request_id = %Uuid::new_v4()))]
    pub async fn tagged_album_edition_until(
        &self,
        actor: &Actor,
        release: &str,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<TaggedAlbumEdition> {
        let started = Instant::now();
        let deadline = tokio::time::Instant::now() + REQUEST_BUDGET;
        tokio::pin!(cancelled);
        let result = tokio::select! {
            biased;
            _ = &mut cancelled => nothing("cancelled"),
            result = tokio::time::timeout_at(deadline, self.find_tagged_edition(actor, release)) => {
                result.unwrap_or_else(|_| nothing("unavailable"))
            }
        };
        tracing::debug!(
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |(_, reason)| *reason),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Tagged album edition lookup completed"
        );
        result.map(|(edition, _)| edition)
    }

    async fn find_tagged_edition(&self, actor: &Actor, release: &str) -> Result<Outcome> {
        let tagged = {
            let mut tx = self.store.pool.begin().await?;
            actor.check_session_on(&mut tx).await?;
            if !release_contains(&mut tx, actor, release, None).await? {
                return Err(Error::NotFound);
            }
            if stored(&mut tx, actor, release).await?.0 != 0 {
                return nothing("owner_choice");
            }
            let settings = policy::read(&mut tx, actor).await?;
            if !settings.enabled
                || !settings.automatic_artist_lookup
                || !policy::selected(&settings, &MetadataProvider::BuiltinMusicBrainz)
            {
                return nothing("consent_off");
            }
            tagged_release(&mut tx, actor, release).await?
        };
        let Some(tagged_release_id) = tagged.release_id else {
            return nothing("not_tagged");
        };
        let album = {
            let mut connection = self.store.pool.acquire().await?;
            releases_by_ids_on(&mut connection, actor, &[release.to_owned()])
                .await?
                .remove(0)
        };
        let Some(anchor) = self.album_anchor(actor, &album, true).await? else {
            return nothing("no_artist");
        };
        let (release_group_id, edition_id) = {
            let Ok(_admission) = self.admit_matching(actor) else {
                return nothing("busy");
            };
            match tokio::time::timeout(
                LOOKUP_DEADLINE,
                self.builtin.release_group_of(&tagged_release_id),
            )
            .await
            {
                Ok(Ok(Some(link))) => (link.release_group_id, link.release_id),
                Ok(Ok(None))
                | Ok(Err(MusicBrainzError::InvalidLookup))
                | Ok(Err(MusicBrainzError::InvalidLookupRequest)) => return nothing("unknown"),
                Ok(Err(_)) | Err(_) => return nothing("unavailable"),
            }
        };
        let mut connection = self.store.pool.acquire().await?;
        actor.check_session_on(&mut connection).await?;
        Ok((
            TaggedAlbumEdition::Found {
                edition: SavedAlbumEdition {
                    recording_id: anchor.recording_id,
                    association_id: anchor.association_id,
                    release_group_id,
                    edition_id,
                },
            },
            "found",
        ))
    }
}
