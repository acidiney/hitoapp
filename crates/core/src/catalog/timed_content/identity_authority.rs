use super::Completion;
use crate::{Actor, Error, Result, identity::recording_access, plugins::Operation};
use serde::Serialize;
use sqlx::SqliteConnection;
use std::{future::Future, time::Duration};
use tracing::Instrument;
use uuid::Uuid;

#[derive(PartialEq, Eq, Serialize, sqlx::FromRow)]
pub(super) struct RecordingTarget {
    library_id: String,
    access_grant: String,
    pub(super) personal: bool,
    metadata_identity: String,
    pin_identity: Option<String>,
}

impl RecordingTarget {
    pub(super) async fn capture(
        connection: &mut SqliteConnection,
        actor: &Actor,
        recording: &str,
    ) -> Result<Self> {
        if !super::valid_id(recording) {
            return Err(Error::Invalid("Use a valid recording identity".into()));
        }
        actor.require_foreground_session()?;
        actor.check_session_on(connection).await?;
        recording_access(connection, actor, recording).await?;
        sqlx::query_as("SELECT recording.library_id,access.id AS access_grant,library.owner_id IS NOT NULL AS personal,
            json_object('binding',recording.metadata_binding_id,'observation',recording.metadata_observation_id,
                'title',recording.title,'artist',recording.artist,'duration',recording.duration_ms,
                'release',recording.release_id,'content',recording.initial_file_checksum,'provenance',recording.provenance,
                'personal_title',personal.override_title,'personal_artist',personal.override_artist,
                'title_operation',personal.title_operation_id,'artist_operation',personal.artist_operation_id) AS metadata_identity,
            CASE WHEN pin.user_id IS NOT NULL THEN json_object('pin_hash',pin.pin_hash,'idle_minutes',pin.idle_minutes) END AS pin_identity
            FROM recordings recording JOIN libraries library ON library.id=recording.library_id
            JOIN library_access_grants access ON access.library_id=recording.library_id AND access.user_id=?
            LEFT JOIN personal_recording_metadata personal ON personal.recording_id=recording.id AND personal.user_id=access.user_id
            LEFT JOIN account_pin_locks pin ON pin.user_id=access.user_id
            WHERE recording.id=? AND (library.owner_id IS NULL OR library.owner_id=access.user_id)")
            .bind(actor.id())
            .bind(recording)
            .fetch_optional(connection)
            .await?
            .ok_or(Error::NotFound)
    }

    pub(super) async fn recheck(
        &self,
        connection: &mut SqliteConnection,
        actor: &Actor,
        recording: &str,
    ) -> Result<()> {
        if Self::capture(connection, actor, recording).await? != *self {
            return Err(Error::Conflict(
                "The recording or its access changed. Review it again.".into(),
            ));
        }
        Ok(())
    }
}

impl crate::Catalog {
    pub(super) async fn check_lyric_target(
        &self,
        actor: &Actor,
        recording: &str,
        target: &RecordingTarget,
        operation: &Operation,
    ) -> Result<()> {
        let mut tx = self.store.pool.begin().await?;
        target.recheck(&mut tx, actor, recording).await?;
        operation.recheck(&mut tx).await?;
        tx.commit().await?;
        Ok(())
    }

    pub(super) async fn lyric_exchange<T>(
        &self,
        actor: &Actor,
        recording: &str,
        target: &RecordingTarget,
        operation: &Operation,
        exchange: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        tokio::pin!(exchange);
        let mut heartbeat = tokio::time::interval(Duration::from_millis(250));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                biased;
                _ = heartbeat.tick() => self.check_lyric_target(actor, recording, target, operation).await?,
                result = &mut exchange => {
                    self.check_lyric_target(actor, recording, target, operation).await?;
                    return result;
                }
            }
        }
    }
}

pub(super) async fn workflow<T>(
    operation: &'static str,
    budget: Duration,
    work: impl Future<Output = Result<T>>,
) -> Result<T> {
    let mut completion = Completion::for_span(
        tracing::debug_span!("recording_lyric_identity", operation, request_id=%Uuid::new_v4()),
        "Recording lyric workflow completed",
    );
    let result = tokio::time::timeout(budget, work)
        .instrument(completion.span.clone())
        .await
        .unwrap_or(Err(Error::Upstream(
            "The lyric workflow did not finish in time",
        )));
    completion.outcome = result
        .as_ref()
        .map(|_| "completed")
        .unwrap_or_else(|error| error.code());
    result
}
