use super::{Catalog, ResetSourceChoice, SourceChoiceResetReceipt, choice_access, recording_id};
use crate::{Actor, Error, Result};
use std::time::Instant;

impl Catalog {
    #[tracing::instrument(level = "info", skip_all, fields(account_id = actor.id(), request_id = %request.request_id))]
    pub async fn reset_source_choice(
        &self,
        actor: &Actor,
        recording: &str,
        request: ResetSourceChoice,
    ) -> Result<SourceChoiceResetReceipt> {
        let started = Instant::now();
        let result = self
            .accept_source_choice_reset(actor, recording, request)
            .await;
        tracing::info!(
            event = "source_choice_reset_finished",
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |_| "accepted"),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Source choice reset finished"
        );
        result
    }

    async fn accept_source_choice_reset(
        &self,
        actor: &Actor,
        recording: &str,
        request: ResetSourceChoice,
    ) -> Result<SourceChoiceResetReceipt> {
        if request.request_id.is_nil() || request.expected_revision < 0 {
            return Err(Error::Invalid(
                "Use a nonempty request identity and a nonnegative source choice revision".into(),
            ));
        }
        let recording = recording_id(recording)?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        choice_access(&mut tx, actor, &recording).await?;
        let choice_used: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM source_choice_receipts WHERE user_id=? AND request_id=?)",
        )
        .bind(actor.id())
        .bind(request.request_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        if choice_used {
            return Err(Error::Conflict(
                "This source choice request was already used for another command".into(),
            ));
        }
        let previous: Option<(String, i64, i64)> = sqlx::query_as(
            "SELECT recording_id,expected_revision,revision FROM source_choice_reset_receipts WHERE user_id=? AND request_id=?",
        )
        .bind(actor.id()).bind(request.request_id.to_string()).fetch_optional(&mut *tx).await?;
        if let Some((previous_recording, expected_revision, revision)) = previous {
            if previous_recording != recording || expected_revision != request.expected_revision {
                return Err(Error::Conflict(
                    "This source choice request was already used for another reset".into(),
                ));
            }
            tx.commit().await?;
            tracing::debug!(
                outcome = "replayed",
                revision,
                "Source choice reset acknowledgement replayed"
            );
            return Ok(SourceChoiceResetReceipt {
                request_id: request.request_id,
                recording_id: recording,
                revision,
            });
        }
        let current: Option<i64> = sqlx::query_scalar(
            "SELECT revision FROM recording_source_choices WHERE user_id=? AND recording_id=?",
        )
        .bind(actor.id())
        .bind(&recording)
        .fetch_optional(&mut *tx)
        .await?;
        if current.unwrap_or(0) != request.expected_revision {
            return Err(Error::Conflict(
                "The source choice changed. Review its current revision before resetting".into(),
            ));
        }
        let revision = request
            .expected_revision
            .checked_add(1)
            .ok_or_else(|| Error::Conflict("Source choice revision is exhausted".into()))?;
        sqlx::query(
            "INSERT INTO recording_source_choices(user_id,recording_id,revision,binding_id) VALUES(?,?,?,NULL)
            ON CONFLICT(user_id,recording_id) DO UPDATE SET revision=excluded.revision,binding_id=NULL",
        )
        .bind(actor.id()).bind(&recording).bind(revision).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO source_choice_reset_receipts(user_id,request_id,recording_id,expected_revision,revision) VALUES(?,?,?,?,?)")
            .bind(actor.id()).bind(request.request_id.to_string()).bind(&recording)
            .bind(request.expected_revision).bind(revision).execute(&mut *tx).await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(SourceChoiceResetReceipt {
            request_id: request.request_id,
            recording_id: recording,
            revision,
        })
    }
}
