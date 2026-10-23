use super::{ArtistIdentityChoiceReceipt, ResetRecordingArtistIdentity, receipts};
use crate::{Actor, Catalog, Error, Result, identity::recording_access};
use std::time::Instant;

impl Catalog {
    #[tracing::instrument(level="info", skip_all, fields(operation="artist_identity_reset", request_id=%request.request_id))]
    pub async fn reset_recording_artist_identity(
        &self,
        actor: &Actor,
        recording: &str,
        request: ResetRecordingArtistIdentity,
    ) -> Result<ArtistIdentityChoiceReceipt> {
        let started = Instant::now();
        let result = self
            .accept_artist_identity_reset(actor, recording, request)
            .await;
        tracing::info!(
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |_| "accepted"),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist identity reset finished"
        );
        result
    }

    async fn accept_artist_identity_reset(
        &self,
        actor: &Actor,
        recording: &str,
        request: ResetRecordingArtistIdentity,
    ) -> Result<ArtistIdentityChoiceReceipt> {
        if request.request_id.is_nil() || request.expected_revision < 0 {
            return Err(Error::Invalid(
                "Use a nonempty request and a nonnegative identity choice revision".into(),
            ));
        }
        let digest = receipts::fingerprint("hito-artist-identity-reset:v1", recording, &request)?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        recording_access(&mut tx, actor, recording).await?;
        if let Some(receipt) = receipts::replay(&mut tx, actor, request.request_id, &digest).await?
        {
            return Ok(receipt);
        }
        let current: Option<i64> = sqlx::query_scalar("SELECT revision FROM recording_artist_identity_choices WHERE user_id=? AND recording_id=?")
            .bind(actor.id()).bind(recording).fetch_optional(&mut *tx).await?;
        if current.unwrap_or(0) != request.expected_revision {
            return Err(Error::Conflict(
                "The artist identity choice changed. Review its current revision".into(),
            ));
        }
        let revision = request.expected_revision.checked_add(1).ok_or_else(|| {
            Error::Conflict("Artist identity choice revision is exhausted".into())
        })?;
        sqlx::query("INSERT INTO recording_artist_identity_choices(user_id,recording_id,revision) VALUES(?,?,?)
            ON CONFLICT(user_id,recording_id) DO UPDATE SET revision=excluded.revision,association_id=NULL,group_id=NULL,
            artist_id=NULL,artist_operation_id=NULL,observation_id=NULL,retired=0,selected_at=unixepoch(),selection_order=NULL")
            .bind(actor.id()).bind(recording).bind(revision).execute(&mut *tx).await?;
        let receipt = ArtistIdentityChoiceReceipt {
            request_id: request.request_id,
            recording_id: recording.into(),
            revision,
            selection: None,
        };
        receipts::save(&mut tx, actor, &digest, request.request_id, &receipt).await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(receipt)
    }
}
