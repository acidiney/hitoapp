use super::{CreditMemberIdentityChoiceReceipt, ResetRecordingArtistIdentity, receipts};
use crate::{Actor, ArtistCatalog, Error, Result, identity::recording_access};
use hito_plugin_sdk::CreditRole;
use std::time::Instant;

impl ArtistCatalog {
    #[tracing::instrument(level="info", skip_all, fields(operation="credit_member_identity_reset", request_id=%request.request_id))]
    pub async fn reset_credit_member_identity(
        &self,
        actor: &Actor,
        recording: &str,
        artist: &str,
        request: ResetRecordingArtistIdentity,
    ) -> Result<CreditMemberIdentityChoiceReceipt> {
        let started = Instant::now();
        let result = self
            .accept_member_identity_reset(actor, recording, artist, request)
            .await;
        tracing::info!(
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |_| "accepted"),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Credit member identity reset finished"
        );
        result
    }

    async fn accept_member_identity_reset(
        &self,
        actor: &Actor,
        recording: &str,
        artist: &str,
        request: ResetRecordingArtistIdentity,
    ) -> Result<CreditMemberIdentityChoiceReceipt> {
        if request.request_id.is_nil() || request.expected_revision < 0 {
            return Err(Error::Invalid(
                "Use a nonempty request and a nonnegative identity choice revision".into(),
            ));
        }
        actor.require_foreground_session()?;
        let digest = receipts::fingerprint(
            "hito-credit-member-identity-reset:v1",
            recording,
            &(artist, &request),
        )?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        recording_access(&mut tx, actor, recording).await?;
        let access: (String, String) = sqlx::query_as(
            "SELECT recording.library_id,grant.id FROM recordings recording
             JOIN library_access_grants grant ON grant.library_id=recording.library_id AND grant.user_id=?
             WHERE recording.id=?",
        )
        .bind(actor.id())
        .bind(recording)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
        let receipt = match receipts::replay::<CreditMemberIdentityChoiceReceipt>(
            &mut tx,
            actor,
            request.request_id,
            &digest,
        )
        .await?
        {
            Some(receipt) => receipt,
            None => {
                let stored: Option<(i64, String)> = sqlx::query_as(
                    "SELECT revision,role FROM recording_artist_member_identity_choices
                     WHERE user_id=? AND recording_id=? AND artist_id=?",
                )
                .bind(actor.id())
                .bind(recording)
                .bind(artist)
                .fetch_optional(&mut *tx)
                .await?;
                let (current_revision, role) = match stored {
                    Some(stored) => stored,
                    None => {
                        let role: String = sqlx::query_scalar(
                            "SELECT member.role FROM account_recording_credit_members member
                             JOIN account_artists artist ON artist.user_id=member.user_id
                                AND artist.id=member.artist_id AND artist.library_id=?
                             WHERE member.user_id=? AND member.recording_id=? AND member.artist_id=?",
                        )
                        .bind(&access.0)
                        .bind(actor.id())
                        .bind(recording)
                        .bind(artist)
                        .fetch_optional(&mut *tx)
                        .await?
                        .ok_or(Error::NotFound)?;
                        (0, role)
                    }
                };
                if current_revision != request.expected_revision {
                    return Err(Error::Conflict(
                        "The artist identity choice changed. Review its current revision".into(),
                    ));
                }
                let member_role = match role.as_str() {
                    "main" => CreditRole::Main,
                    "featured" => CreditRole::Featured,
                    _ => {
                        return Err(Error::Internal(
                            "Stored artist credit member is invalid".into(),
                        ));
                    }
                };
                let revision = request.expected_revision.checked_add(1).ok_or_else(|| {
                    Error::Conflict("Artist identity choice revision is exhausted".into())
                })?;
                sqlx::query(
                    "INSERT INTO recording_artist_member_identity_choices(user_id,recording_id,artist_id,role,revision)
                     VALUES(?,?,?,?,?) ON CONFLICT(user_id,recording_id,artist_id) DO UPDATE SET
                        revision=excluded.revision,association_id=NULL,group_id=NULL,artist_operation_id=NULL,
                        observation_id=NULL,retired=0,selected_at=unixepoch(),selection_order=NULL",
                )
                .bind(actor.id())
                .bind(recording)
                .bind(artist)
                .bind(&role)
                .bind(revision)
                .execute(&mut *tx)
                .await?;
                let receipt = CreditMemberIdentityChoiceReceipt {
                    request_id: request.request_id,
                    recording_id: recording.into(),
                    artist_id: artist.into(),
                    role: member_role,
                    revision,
                    selection: None,
                };
                receipts::save(&mut tx, actor, &digest, request.request_id, &receipt).await?;
                receipt
            }
        };
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        let mut fresh = self.store.pool.acquire().await?;
        actor.check_session_on(&mut fresh).await?;
        #[cfg(feature = "test-support")]
        tracing::debug!(
            target: "hito_core::authority_snapshot_test",
            point = "credit_member_identity_reset_fresh_session",
            snapshot_current = true,
            transaction_open = <sqlx::sqlite::SqliteTransactionManager as sqlx::TransactionManager>::get_transaction_depth(&*fresh) > 0,
            "Owned member reset fresh session checkpoint"
        );
        let access_result: Result<()> = async {
            recording_access(&mut fresh, actor, recording).await?;
            let current: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM recordings recording
                 JOIN library_access_grants grant ON grant.library_id=recording.library_id AND grant.user_id=?
                 WHERE recording.id=? AND recording.library_id=? AND grant.id=?)",
            )
            .bind(actor.id())
            .bind(recording)
            .bind(&access.0)
            .bind(&access.1)
            .fetch_one(&mut *fresh)
            .await?;
            if !current {
                return Err(Error::NotFound);
            }
            Ok(())
        }
        .await;
        actor.check_session_on(&mut fresh).await?;
        access_result?;
        Ok(receipt)
    }
}
