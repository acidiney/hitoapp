use super::super::{associations, authority::Credit, provider::Connection, review};
use super::receipts;
use super::*;
use crate::{Actor, Catalog, Error, identity::recording_access};
use sqlx::SqliteConnection;
use std::time::Instant;
use uuid::Uuid;

impl Catalog {
    #[tracing::instrument(level="debug", skip_all, fields(operation="artist_identity_choice_read", request_id=%Uuid::new_v4()))]
    pub async fn recording_artist_identity_choice(
        &self,
        actor: &Actor,
        recording: &str,
    ) -> Result<RecordingArtistIdentityChoice> {
        let started = Instant::now();
        let result = self.read_artist_identity_choice(actor, recording).await;
        tracing::debug!(
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |_| "success"),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist identity choice loaded"
        );
        result
    }

    async fn read_artist_identity_choice(
        &self,
        actor: &Actor,
        recording: &str,
    ) -> Result<RecordingArtistIdentityChoice> {
        let mut tx = self.store.pool.begin().await?;
        actor.check_session_on(&mut tx).await?;
        recording_access(&mut tx, actor, recording).await?;
        let row: Option<(i64, Option<String>, Option<String>, bool)> = sqlx::query_as(
            "SELECT revision,group_id,association_id,retired FROM recording_artist_identity_choices WHERE user_id=? AND recording_id=?")
            .bind(actor.id()).bind(recording).fetch_optional(&mut *tx).await?;
        let (revision, state) = match row {
            Some((revision, Some(group_id), Some(association_id), retired)) => {
                let selection = ArtistIdentitySelection {
                    group_id,
                    association_id,
                };
                (
                    revision,
                    if retired {
                        RecordingArtistIdentityState::NeedsReview(selection)
                    } else {
                        RecordingArtistIdentityState::Selected(selection)
                    },
                )
            }
            Some((revision, _, _, _)) => (revision, RecordingArtistIdentityState::Unselected),
            None => (0, RecordingArtistIdentityState::Unselected),
        };
        tx.commit().await?;
        Ok(RecordingArtistIdentityChoice {
            recording_id: recording.into(),
            revision,
            state,
        })
    }

    #[tracing::instrument(level="info", skip_all, fields(operation="artist_identity_choose", request_id=%request.request_id))]
    pub async fn choose_recording_artist_identity(
        &self,
        actor: &Actor,
        recording: &str,
        request: ChooseRecordingArtistIdentity,
    ) -> Result<ArtistIdentityChoiceReceipt> {
        let started = Instant::now();
        let result = self.accept_artist_identity(actor, recording, request).await;
        tracing::info!(
            outcome = result.as_ref().map_or_else(|e| e.code(), |_| "accepted"),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist identity choice finished"
        );
        result
    }

    async fn accept_artist_identity(
        &self,
        actor: &Actor,
        recording: &str,
        request: ChooseRecordingArtistIdentity,
    ) -> Result<ArtistIdentityChoiceReceipt> {
        if request.request_id.is_nil()
            || request.association_id.is_nil()
            || request.expected_revision < 0
        {
            return Err(Error::Invalid(
                "Use a nonempty request and association with a nonnegative revision".into(),
            ));
        }
        if request.context.account_id != actor.id() || request.context.recording_id != recording {
            return Err(Error::NotFound);
        }
        let digest = receipts::fingerprint("hito-artist-identity-choose:v1", recording, &request)?;
        {
            let mut connection = self.store.pool.acquire().await?;
            actor.check_session_on(&mut connection).await?;
            recording_access(&mut connection, actor, recording).await?;
            if let Some(receipt) =
                receipts::replay(&mut connection, actor, request.request_id, &digest).await?
            {
                return Ok(receipt);
            }
        }
        let connection = Connection::open(self, actor, &request.context.provider).await?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        recording_access(&mut tx, actor, recording).await?;
        if let Some(receipt) = receipts::replay(&mut tx, actor, request.request_id, &digest).await?
        {
            return Ok(receipt);
        }
        let current:Option<i64>=sqlx::query_scalar("SELECT revision FROM recording_artist_identity_choices WHERE user_id=? AND recording_id=?")
            .bind(actor.id()).bind(recording).fetch_optional(&mut *tx).await?;
        if current.unwrap_or(0) != request.expected_revision {
            return Err(Error::Conflict(
                "The artist identity choice changed. Review its current revision".into(),
            ));
        }
        if !review::recheck_on(&mut tx, actor, &request.context, Some(&connection)).await? {
            return Err(Error::Conflict("The artist identity review changed".into()));
        }
        let association = associations::load(
            &mut tx,
            actor,
            recording,
            &request.association_id.to_string(),
        )
        .await?;
        let credit = Credit::capture(&mut tx, actor, recording).await?;
        if !credit.matches(&association) || association.provider != request.context.provider {
            return Err(Error::Conflict(
                "The reviewed artist association changed".into(),
            ));
        }
        let revision = request.expected_revision.checked_add(1).ok_or_else(|| {
            Error::Conflict("Artist identity choice revision is exhausted".into())
        })?;
        let (kind, installation) = super::super::authority::provider_columns(&association.provider);
        let group:String=sqlx::query_scalar("INSERT INTO account_artist_identity_groups(id,user_id,provider_kind,plugin_installation_id,provider_artist_id)
            VALUES(?,?,?,?,?) ON CONFLICT(user_id,provider_key,provider_artist_id) DO UPDATE SET provider_artist_id=excluded.provider_artist_id RETURNING id")
            .bind(Uuid::new_v4().to_string()).bind(actor.id()).bind(kind).bind(installation).bind(&association.provider_artist_id).fetch_one(&mut *tx).await?;
        let receipt = ArtistIdentityChoiceReceipt {
            request_id: request.request_id,
            recording_id: recording.into(),
            revision,
            selection: Some(ArtistIdentitySelection {
                group_id: group.clone(),
                association_id: association.id.clone(),
            }),
        };
        let order = receipts::save(&mut tx, actor, &digest, request.request_id, &receipt).await?;
        store(
            &mut tx,
            actor,
            recording,
            revision,
            &association.id,
            &group,
            &credit,
            order,
        )
        .await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn store(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
    revision: i64,
    association: &str,
    group: &str,
    credit: &Credit,
    order: i64,
) -> Result<()> {
    sqlx::query("INSERT INTO recording_artist_identity_choices(user_id,recording_id,revision,association_id,group_id,artist_id,artist_operation_id,observation_id,selection_order)
        VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(user_id,recording_id) DO UPDATE SET revision=excluded.revision,association_id=excluded.association_id,
        group_id=excluded.group_id,artist_id=excluded.artist_id,artist_operation_id=excluded.artist_operation_id,observation_id=excluded.observation_id,retired=0,selected_at=unixepoch(),selection_order=excluded.selection_order")
        .bind(actor.id()).bind(recording).bind(revision).bind(association).bind(group).bind(&credit.artist_id)
        .bind(&credit.credit_operation).bind(&credit.observation_id).bind(order).execute(connection).await?;
    Ok(())
}
