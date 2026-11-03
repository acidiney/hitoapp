use super::{
    AlbumEditionTrackListAvailability, ReadAlbumEditionTrackList,
    album_editions::readable_association, authority::provider_columns, policy,
};
use crate::{Actor, ArtistCatalog, Error, Result, catalog::releases::release_contains};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use std::{future::Future, time::Instant};
use utoipa::ToSchema;
use uuid::Uuid;

const MAX_IDENTITY_BYTES: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
pub struct SavedAlbumEdition {
    pub recording_id: String,
    pub association_id: String,
    pub release_group_id: String,
    pub edition_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum AlbumEditionChoiceState {
    Unset,
    Ready { edition: SavedAlbumEdition },
    NeedsChoice,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ToSchema)]
pub struct AlbumEditionChoice {
    pub revision: i64,
    pub state: AlbumEditionChoiceState,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChooseAlbumEdition {
    pub request_id: Uuid,
    pub expected_revision: i64,
    pub recording_id: String,
    pub association_id: String,
    pub release_group_id: String,
    pub edition_id: String,
}

#[derive(Clone, Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ClearAlbumEdition {
    pub request_id: Uuid,
    pub expected_revision: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize, ToSchema)]
pub struct AlbumEditionChoiceReceipt {
    pub request_id: Uuid,
    pub release_id: String,
    pub revision: i64,
    #[schema(required = true)]
    pub edition: Option<SavedAlbumEdition>,
}

type StoredChoice = (
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn fingerprint(command: &str, release: &str, request: &impl Serialize) -> Result<String> {
    let encoded = serde_json::to_vec(&(command, release, request))
        .map_err(|_| Error::Internal("Album edition command could not be encoded".into()))?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

fn bounded(value: &str) -> bool {
    (1..=MAX_IDENTITY_BYTES).contains(&value.len())
}

impl ArtistCatalog {
    pub(crate) async fn saved_album_edition(
        &self,
        actor: &Actor,
        release: &str,
    ) -> Result<AlbumEditionChoice> {
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        let (revision, saved) = stored(&mut tx, actor, release).await?;
        if saved.is_none() && !release_contains(&mut tx, actor, release, None).await? {
            return Err(Error::NotFound);
        }
        let state = match saved {
            None => AlbumEditionChoiceState::Unset,
            Some(saved) => {
                let settings = policy::read(&mut tx, actor).await?;
                let readable = release_contains(&mut tx, actor, release, Some(&saved.recording_id))
                    .await?
                    && readable_association(
                        &mut tx,
                        actor,
                        &settings,
                        &saved.recording_id,
                        &saved.association_id,
                    )
                    .await?
                    .is_some();
                if readable {
                    AlbumEditionChoiceState::Ready { edition: saved }
                } else {
                    AlbumEditionChoiceState::NeedsChoice
                }
            }
        };
        tx.commit().await?;
        Ok(AlbumEditionChoice { revision, state })
    }

    #[tracing::instrument(level = "debug", skip_all, fields(operation = "saved_album_edition_track_list", request_id = %Uuid::new_v4()))]
    pub async fn saved_album_edition_track_list_until(
        &self,
        actor: &Actor,
        release: &str,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<AlbumEditionTrackListAvailability> {
        self.bounded_album_read(
            cancelled,
            self.read_saved_album_edition(actor, release, None),
        )
        .await
    }

    pub(super) async fn read_saved_album_edition(
        &self,
        actor: &Actor,
        release: &str,
        release_group: Option<&str>,
    ) -> Result<AlbumEditionTrackListAvailability> {
        let choice = self.saved_album_edition(actor, release).await?;
        let AlbumEditionChoiceState::Ready { edition } = choice.state else {
            return Ok(AlbumEditionTrackListAvailability::NeedsChoice);
        };
        if release_group.is_some_and(|group| group != edition.release_group_id) {
            return Ok(AlbumEditionTrackListAvailability::NeedsChoice);
        }
        self.read_album_edition(
            actor,
            release,
            ReadAlbumEditionTrackList {
                recording_id: edition.recording_id,
                association_id: edition.association_id,
                release_group_id: edition.release_group_id,
                edition_id: edition.edition_id,
            },
        )
        .await
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "album_edition_choose", request_id = %request.request_id))]
    pub async fn choose_album_edition(
        &self,
        actor: &Actor,
        release: &str,
        request: ChooseAlbumEdition,
    ) -> Result<AlbumEditionChoiceReceipt> {
        let started = Instant::now();
        let result = self.accept_album_edition(actor, release, request).await;
        tracing::info!(
            outcome = result.as_ref().map_or_else(|e| e.code(), |_| "accepted"),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Album edition choice finished"
        );
        result
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "album_edition_clear", request_id = %request.request_id))]
    pub async fn clear_album_edition(
        &self,
        actor: &Actor,
        release: &str,
        request: ClearAlbumEdition,
    ) -> Result<AlbumEditionChoiceReceipt> {
        let started = Instant::now();
        let result = self
            .accept_album_edition_clear(actor, release, request)
            .await;
        tracing::info!(
            outcome = result.as_ref().map_or_else(|e| e.code(), |_| "accepted"),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Album edition clear finished"
        );
        result
    }

    async fn accept_album_edition(
        &self,
        actor: &Actor,
        release: &str,
        request: ChooseAlbumEdition,
    ) -> Result<AlbumEditionChoiceReceipt> {
        if request.request_id.is_nil()
            || request.expected_revision < 0
            || !bounded(&request.release_group_id)
            || !bounded(&request.edition_id)
        {
            return Err(Error::Invalid(
                "Use a nonempty request, a nonnegative revision and bounded catalog identities"
                    .into(),
            ));
        }
        let digest = fingerprint("hito-album-edition-choose:v1", release, &request)?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        if let Some(receipt) = replay(&mut tx, actor, request.request_id, &digest).await? {
            return Ok(receipt);
        }
        if !release_contains(&mut tx, actor, release, Some(&request.recording_id)).await? {
            return Err(Error::NotFound);
        }
        let (current, _) = stored(&mut tx, actor, release).await?;
        if current != request.expected_revision {
            return Err(changed());
        }
        let settings = policy::read(&mut tx, actor).await?;
        let association = readable_association(
            &mut tx,
            actor,
            &settings,
            &request.recording_id,
            &request.association_id,
        )
        .await?
        .ok_or_else(|| {
            Error::Conflict("The reviewed artist is not readable. Review the album again".into())
        })?;
        let revision = next(current)?;
        let (kind, installation) = provider_columns(&association.provider);
        sqlx::query("INSERT INTO album_edition_choices(user_id,release_id,revision,recording_id,association_id,provider_kind,plugin_installation_id,release_group_id,edition_id)
            VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(user_id,release_id) DO UPDATE SET revision=excluded.revision,recording_id=excluded.recording_id,
            association_id=excluded.association_id,provider_kind=excluded.provider_kind,plugin_installation_id=excluded.plugin_installation_id,
            release_group_id=excluded.release_group_id,edition_id=excluded.edition_id,updated_at=unixepoch()")
            .bind(actor.id()).bind(release).bind(revision).bind(&request.recording_id).bind(&association.id)
            .bind(kind).bind(installation).bind(&request.release_group_id).bind(&request.edition_id)
            .execute(&mut *tx).await?;
        let receipt = AlbumEditionChoiceReceipt {
            request_id: request.request_id,
            release_id: release.into(),
            revision,
            edition: Some(SavedAlbumEdition {
                recording_id: request.recording_id,
                association_id: association.id,
                release_group_id: request.release_group_id,
                edition_id: request.edition_id,
            }),
        };
        save(&mut tx, actor, &digest, &receipt).await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }

    async fn accept_album_edition_clear(
        &self,
        actor: &Actor,
        release: &str,
        request: ClearAlbumEdition,
    ) -> Result<AlbumEditionChoiceReceipt> {
        if request.request_id.is_nil() || request.expected_revision < 0 {
            return Err(Error::Invalid(
                "Use a nonempty request and a nonnegative revision".into(),
            ));
        }
        let digest = fingerprint("hito-album-edition-clear:v1", release, &request)?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        if let Some(receipt) = replay(&mut tx, actor, request.request_id, &digest).await? {
            return Ok(receipt);
        }
        let (current, _) = stored(&mut tx, actor, release).await?;
        if current == 0 && !release_contains(&mut tx, actor, release, None).await? {
            return Err(Error::NotFound);
        }
        if current != request.expected_revision {
            return Err(changed());
        }
        let revision = next(current)?;
        sqlx::query("INSERT INTO album_edition_choices(user_id,release_id,revision) VALUES(?,?,?)
            ON CONFLICT(user_id,release_id) DO UPDATE SET revision=excluded.revision,recording_id=NULL,association_id=NULL,
            provider_kind=NULL,plugin_installation_id=NULL,release_group_id=NULL,edition_id=NULL,updated_at=unixepoch()")
            .bind(actor.id()).bind(release).bind(revision).execute(&mut *tx).await?;
        let receipt = AlbumEditionChoiceReceipt {
            request_id: request.request_id,
            release_id: release.into(),
            revision,
            edition: None,
        };
        save(&mut tx, actor, &digest, &receipt).await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }
}

fn changed() -> Error {
    Error::Conflict("The saved album edition changed. Review its current revision".into())
}

fn next(revision: i64) -> Result<i64> {
    revision
        .checked_add(1)
        .ok_or_else(|| Error::Conflict("Album edition revision is exhausted".into()))
}

pub(super) async fn stored(
    connection: &mut SqliteConnection,
    actor: &Actor,
    release: &str,
) -> Result<(i64, Option<SavedAlbumEdition>)> {
    let row: Option<StoredChoice> = sqlx::query_as("SELECT revision,recording_id,association_id,release_group_id,edition_id FROM album_edition_choices WHERE user_id=? AND release_id=?")
        .bind(actor.id()).bind(release).fetch_optional(connection).await?;
    Ok(match row {
        Some((
            revision,
            Some(recording_id),
            Some(association_id),
            Some(release_group_id),
            Some(edition_id),
        )) => (
            revision,
            Some(SavedAlbumEdition {
                recording_id,
                association_id,
                release_group_id,
                edition_id,
            }),
        ),
        Some((revision, ..)) => (revision, None),
        None => (0, None),
    })
}

async fn replay(
    connection: &mut SqliteConnection,
    actor: &Actor,
    request_id: Uuid,
    digest: &str,
) -> Result<Option<AlbumEditionChoiceReceipt>> {
    let previous: Option<(String, String)> = sqlx::query_as(
        "SELECT request_digest,receipt FROM album_edition_choice_receipts WHERE user_id=? AND request_id=?",
    )
    .bind(actor.id())
    .bind(request_id.to_string())
    .fetch_optional(connection)
    .await?;
    let Some((previous, receipt)) = previous else {
        return Ok(None);
    };
    if previous != digest {
        return Err(Error::Conflict(
            "This album edition request was already used with different contents".into(),
        ));
    }
    tracing::debug!(
        outcome = "replayed",
        "Album edition acknowledgement replayed"
    );
    serde_json::from_str(&receipt)
        .map(Some)
        .map_err(|_| Error::Internal("Stored album edition receipt is invalid".into()))
}

async fn save(
    connection: &mut SqliteConnection,
    actor: &Actor,
    digest: &str,
    receipt: &AlbumEditionChoiceReceipt,
) -> Result<()> {
    let encoded = serde_json::to_string(receipt)
        .map_err(|_| Error::Internal("Album edition receipt could not be encoded".into()))?;
    sqlx::query("INSERT INTO album_edition_choice_receipts(user_id,request_id,request_digest,receipt) VALUES(?,?,?,?)")
        .bind(actor.id()).bind(receipt.request_id.to_string()).bind(digest).bind(encoded)
        .execute(connection).await?;
    Ok(())
}
