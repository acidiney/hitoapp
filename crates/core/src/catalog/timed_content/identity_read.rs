use super::{
    Completion, authority::Target, identity_authority::RecordingTarget, identity_types::*,
};
use crate::{
    Actor, Catalog, Error, Result,
    plugins::{LyricsDocumentResponse, Operation},
};
use hito_plugin_sdk::{Capability, LyricsRequest, ProviderRecordingRef};
use sha2::{Digest, Sha256};
use sqlx::SqliteConnection;
use std::{future::Future, time::Duration};
use tracing::Instrument;
use uuid::Uuid;

#[derive(PartialEq, Eq, sqlx::FromRow)]
struct ReviewedTarget {
    id: String,
    revision: i64,
    installation_id: String,
    provider_recording_id: String,
    provider_revision: Option<String>,
}

impl ReviewedTarget {
    async fn capture(
        connection: &mut SqliteConnection,
        actor: &Actor,
        recording: &str,
        id: &str,
        revision: i64,
    ) -> Result<Self> {
        if !super::valid_id(id) || revision < 1 {
            return Err(Error::Invalid(
                "Use a current reviewed lyric identity".into(),
            ));
        }
        sqlx::query_as("SELECT identity.id,identity.revision,identity.installation_id,identity.provider_recording_id,identity.provider_revision
            FROM recording_lyric_identities identity JOIN plugin_installations installation ON installation.id=identity.installation_id AND installation.user_id=identity.user_id AND installation.active=1
            JOIN plugin_services service ON service.id=installation.service_id AND service.active=1
            WHERE identity.id=? AND identity.revision=? AND identity.user_id=? AND identity.recording_id=?")
            .bind(id).bind(revision).bind(actor.id()).bind(recording).fetch_optional(connection).await?.ok_or(Error::NotFound)
    }
}

enum SelectedTarget {
    Observation(Target),
    Reviewed(ReviewedTarget),
}
impl SelectedTarget {
    async fn capture(
        connection: &mut SqliteConnection,
        actor: &Actor,
        recording: &str,
        selector: &RecordingLyricSelector,
    ) -> Result<Self> {
        match selector {
            RecordingLyricSelector::Observation { observation_id } => {
                if !super::valid_id(observation_id) {
                    return Err(Error::Invalid("Use a valid retained observation".into()));
                }
                Ok(Self::Observation(
                    Target::capture(connection, actor, recording, observation_id).await?,
                ))
            }
            RecordingLyricSelector::Reviewed {
                identity_id,
                identity_revision,
            } => Ok(Self::Reviewed(
                ReviewedTarget::capture(
                    connection,
                    actor,
                    recording,
                    identity_id,
                    *identity_revision,
                )
                .await?,
            )),
        }
    }
    fn installation(&self) -> &str {
        match self {
            Self::Observation(target) => &target.installation_id,
            Self::Reviewed(target) => &target.installation_id,
        }
    }
    fn reference(&self) -> ProviderRecordingRef {
        match self {
            Self::Observation(target) => target.reference(),
            Self::Reviewed(target) => ProviderRecordingRef {
                id: target.provider_recording_id.clone(),
                revision: target.provider_revision.clone(),
            },
        }
    }
    async fn recheck(
        &self,
        connection: &mut SqliteConnection,
        actor: &Actor,
        recording: &str,
    ) -> Result<()> {
        match self {
            Self::Observation(target) => target.recheck(connection, actor, recording).await,
            Self::Reviewed(target) => {
                if ReviewedTarget::capture(
                    connection,
                    actor,
                    recording,
                    &target.id,
                    target.revision,
                )
                .await?
                    != *target
                {
                    return Err(Error::Conflict(
                        "The reviewed lyric identity changed. Reload it before continuing.".into(),
                    ));
                }
                Ok(())
            }
        }
    }
}

impl Catalog {
    pub async fn selected_recording_lyrics_until(
        &self,
        actor: &Actor,
        recording: &str,
        request: ReadSelectedRecordingLyrics,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<SelectedLyricsLookup> {
        let mut completion = Completion::for_span(
            tracing::debug_span!("selected_recording_lyrics", operation="selected_recording_lyrics", request_id=%Uuid::new_v4()),
            "Selected recording lyric lookup completed",
        );
        tokio::pin!(cancelled);
        let result = async { tokio::select! {
            biased;
            _ = &mut cancelled => Ok(SelectedLyricsLookup::Cancelled),
            result = tokio::time::timeout(super::LOOKUP_BUDGET, self.read_selected_lyrics(actor, recording, request)) => result.unwrap_or(Ok(SelectedLyricsLookup::Unavailable)),
        } }.instrument(completion.span.clone()).await;
        completion.outcome = result
            .as_ref()
            .map(SelectedLyricsLookup::outcome)
            .unwrap_or_else(|error| error.code());
        result
    }

    async fn read_selected_lyrics(
        &self,
        actor: &Actor,
        recording: &str,
        request: ReadSelectedRecordingLyrics,
    ) -> Result<SelectedLyricsLookup> {
        if request.installation_revision < 1 {
            return Err(Error::Invalid(
                "Use a current lyric installation revision".into(),
            ));
        }
        let (recording_target, selected) = {
            let mut tx = self.store.pool.begin().await?;
            let target = RecordingTarget::capture(&mut tx, actor, recording).await?;
            let selected =
                SelectedTarget::capture(&mut tx, actor, recording, &request.selector).await?;
            tx.commit().await?;
            (target, selected)
        };
        let operation = self
            .plugins
            .operation(
                actor,
                selected.installation(),
                request.installation_revision,
                &[Capability::Lyrics],
            )
            .await?;
        let provider_recording = selected.reference();
        let provider_request = LyricsRequest {
            recording: provider_recording.clone(),
        };
        let exchange = operation.selected_lyrics_checked(&provider_request, || {
            self.check_selected_lyrics(actor, recording, &recording_target, &selected, &operation)
        });
        tokio::pin!(exchange);
        let mut heartbeat = tokio::time::interval(Duration::from_millis(250));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let result = loop {
            tokio::select! {
                biased;
                _=heartbeat.tick()=>self.check_selected_lyrics(actor, recording, &recording_target, &selected, &operation).await?,
                result=&mut exchange=>break result,
            }
        };
        self.check_selected_lyrics(actor, recording, &recording_target, &selected, &operation)
            .await?;
        match result {
            Ok(Some(LyricsDocumentResponse::V1(document))) => Ok(SelectedLyricsLookup::ReadyV1 {
                recording_id: recording.into(),
                selector: request.selector,
                provider_recording,
                document_digest: digest(&document)?,
                document: Box::new(document),
            }),
            Ok(Some(LyricsDocumentResponse::V2(document))) => Ok(SelectedLyricsLookup::ReadyV2 {
                recording_id: recording.into(),
                selector: request.selector,
                provider_recording,
                document_digest: digest(&document)?,
                document: Box::new(document),
            }),
            Ok(None) => Ok(SelectedLyricsLookup::Unsupported),
            Err(Error::Upstream(_)) => Ok(SelectedLyricsLookup::Unavailable),
            Err(error) => Err(error),
        }
    }

    async fn check_selected_lyrics(
        &self,
        actor: &Actor,
        recording: &str,
        target: &RecordingTarget,
        selected: &SelectedTarget,
        operation: &Operation,
    ) -> Result<()> {
        let mut tx = self.store.pool.begin().await?;
        target.recheck(&mut tx, actor, recording).await?;
        selected.recheck(&mut tx, actor, recording).await?;
        operation.recheck(&mut tx).await?;
        #[cfg(feature = "test-support")]
        tracing::debug!(
            target: "hito_core::authority_snapshot_test",
            point = "selected_lyric_authority",
            snapshot_current = true,
            transaction_open = <sqlx::sqlite::SqliteTransactionManager as sqlx::TransactionManager>::get_transaction_depth(&*tx) > 0,
            "Owned authority snapshot checkpoint"
        );
        tx.commit().await?;
        operation.check_current().await?;
        let mut connection = self.store.pool.acquire().await?;
        target.recheck(&mut connection, actor, recording).await?;
        selected.recheck(&mut connection, actor, recording).await
    }
}

fn digest(document: &impl serde::Serialize) -> Result<String> {
    let encoded = serde_json::to_vec(document)
        .map_err(|_| Error::Internal("Validated lyrics could not be encoded".into()))?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}
