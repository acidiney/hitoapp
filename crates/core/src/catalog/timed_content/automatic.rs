use super::{
    Completion,
    automatic_matching::AutomaticMetadata,
    automatic_types::AutomaticLyricsLookup,
    identity_authority::RecordingTarget,
    identity_services::{REQUIRED, approved_services},
};
use crate::{Actor, Catalog, Error, Result, plugins::LyricsDocumentResponse};
use hito_plugin_sdk::{LyricsRequest, ProviderRecordingRef, RecordingRequest};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use std::{future::Future, time::Duration};
use tracing::Instrument;
use uuid::Uuid;

impl AutomaticMetadata {
    async fn capture(
        connection: &mut SqliteConnection,
        actor: &Actor,
        recording: &str,
    ) -> Result<Self> {
        sqlx::query_as("SELECT COALESCE(personal.override_title,recording.title) AS title,
            COALESCE(personal.override_artist,recording.artist) AS artist,recording.duration_ms,release.title AS album
            FROM recordings recording LEFT JOIN releases release ON release.id=recording.release_id
            LEFT JOIN personal_recording_metadata personal ON personal.recording_id=recording.id AND personal.user_id=?
            WHERE recording.id=?")
            .bind(actor.id()).bind(recording).fetch_optional(connection).await?.ok_or(Error::NotFound)
    }
}

impl Catalog {
    pub async fn automatic_recording_lyrics_until(
        &self,
        actor: &Actor,
        recording: &str,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<AutomaticLyricsLookup> {
        let mut completion = Completion::for_span(
            tracing::debug_span!("automatic_recording_lyrics", operation="automatic_recording_lyrics", request_id=%Uuid::new_v4()),
            "Automatic recording lyric lookup completed",
        );
        tokio::pin!(cancelled);
        let result = async { tokio::select! {
            biased;
            _ = &mut cancelled => Ok(AutomaticLyricsLookup::Cancelled),
            result = tokio::time::timeout(super::LOOKUP_BUDGET, self.read_automatic_lyrics(actor, recording)) => result.unwrap_or(Ok(AutomaticLyricsLookup::Unavailable)),
        } }.instrument(completion.span.clone()).await;
        completion.outcome = result
            .as_ref()
            .map(AutomaticLyricsLookup::outcome)
            .unwrap_or_else(|error| error.code());
        result
    }

    async fn read_automatic_lyrics(
        &self,
        actor: &Actor,
        recording: &str,
    ) -> Result<AutomaticLyricsLookup> {
        let (target, metadata, service) = {
            let mut tx = self.store.pool.begin().await?;
            let target = RecordingTarget::capture(&mut tx, actor, recording).await?;
            let metadata = AutomaticMetadata::capture(&mut tx, actor, recording).await?;
            let service = approved_services(&mut tx, actor).await?.into_iter().next();
            tx.commit().await?;
            (target, metadata, service)
        };
        let Some(service) = service else {
            target
                .recheck(&mut *self.store.pool.acquire().await?, actor, recording)
                .await?;
            return Ok(AutomaticLyricsLookup::NoServices);
        };
        let Some(query) = metadata.query() else {
            target
                .recheck(&mut *self.store.pool.acquire().await?, actor, recording)
                .await?;
            return Ok(AutomaticLyricsLookup::InsufficientMetadata);
        };
        let operation = self
            .plugins
            .operation(
                actor,
                &service.installation_id,
                service.installation_revision,
                REQUIRED,
            )
            .await?;
        let check = || async {
            let mut tx = self.store.pool.begin().await?;
            target.recheck(&mut tx, actor, recording).await?;
            operation.recheck(&mut tx).await?;
            let current = AutomaticMetadata::capture(&mut tx, actor, recording).await?;
            if current != metadata {
                return Err(Error::Conflict(
                    "The recording metadata changed. Try again.".into(),
                ));
            }
            #[cfg(feature = "test-support")]
            tracing::debug!(
                target: "hito_core::authority_snapshot_test",
                point = "automatic_lyric_authority",
                snapshot_current = true,
                transaction_open = <sqlx::sqlite::SqliteTransactionManager as sqlx::TransactionManager>::get_transaction_depth(&*tx) > 0,
                "Owned authority snapshot checkpoint"
            );
            tx.commit().await?;
            operation.check_current().await?;
            let mut connection = self.store.pool.acquire().await?;
            target.recheck(&mut connection, actor, recording).await?;
            let current = AutomaticMetadata::capture(&mut connection, actor, recording).await?;
            if current != metadata {
                return Err(Error::Conflict(
                    "The recording metadata changed. Try again.".into(),
                ));
            }
            Ok(())
        };
        let mut request = operation.lyric_request()?;
        let exchange = async {
            let page = request.search_checked(&query, check).await?;
            if page.next_cursor.is_some() {
                return Ok(AutomaticLyricsLookup::Ambiguous);
            }
            let mut plain = None;
            for candidate in page
                .items
                .into_iter()
                .filter(|candidate| metadata.matches(candidate))
            {
                let details = request
                    .metadata_checked(
                        &RecordingRequest {
                            recording_id: candidate.id.clone(),
                        },
                        check,
                    )
                    .await?;
                if details.recording.id != candidate.id
                    || details.recording.revision != candidate.revision
                {
                    return Err(Error::Upstream(
                        "The lyric provider identity changed during lookup",
                    ));
                }
                if !metadata.matches_metadata(&details) {
                    continue;
                }
                let provider_recording = ProviderRecordingRef {
                    id: details.recording.id,
                    revision: details.recording.revision,
                };
                let document = request
                    .document_checked(
                        &LyricsRequest {
                            recording: provider_recording.clone(),
                        },
                        check,
                    )
                    .await?;
                let Some(document) = document else {
                    return Ok(AutomaticLyricsLookup::Unsupported);
                };
                let timed = match &document {
                    LyricsDocumentResponse::V1(document) => document.lyrics.is_some(),
                    LyricsDocumentResponse::V2(document) => {
                        document.lyrics.as_ref().is_some_and(|lyrics| {
                            matches!(
                                lyrics.content,
                                hito_plugin_sdk::LyricContent::Anchored { .. }
                            )
                        })
                    }
                };
                if !timed && plain.is_some() {
                    continue;
                }
                let result = match document {
                    LyricsDocumentResponse::V1(document) if document.lyrics.is_some() => {
                        AutomaticLyricsLookup::ReadyV1 {
                            recording_id: recording.into(),
                            installation_id: service.installation_id.clone(),
                            installation_revision: service.installation_revision,
                            label: service.label.clone(),
                            provider_recording,
                            document_digest: digest(&document)?,
                            document: Box::new(document),
                        }
                    }
                    LyricsDocumentResponse::V2(document) if document.lyrics.is_some() => {
                        AutomaticLyricsLookup::ReadyV2 {
                            recording_id: recording.into(),
                            installation_id: service.installation_id.clone(),
                            installation_revision: service.installation_revision,
                            label: service.label.clone(),
                            provider_recording,
                            document_digest: digest(&document)?,
                            document: Box::new(document),
                        }
                    }
                    _ => continue,
                };
                if timed {
                    return Ok(result);
                }
                plain = Some(result);
            }
            Ok(plain.unwrap_or(AutomaticLyricsLookup::NotFound))
        };
        tokio::pin!(exchange);
        let mut heartbeat = tokio::time::interval(Duration::from_millis(250));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let result = loop {
            tokio::select! {
                biased;
                _=heartbeat.tick()=>check().await?,
                result=&mut exchange=>break result,
            }
        };
        check().await?;
        match result {
            Err(Error::Upstream(_) | Error::UpstreamNotice(_)) => {
                Ok(AutomaticLyricsLookup::Unavailable)
            }
            result => result,
        }
    }
}

fn digest(document: &impl serde::Serialize) -> Result<String> {
    let encoded = serde_json::to_vec(document)
        .map_err(|_| Error::Internal("Validated lyrics could not be encoded".into()))?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}
