mod authority;
mod types;

pub use types::{LyricsLookup, ReadRecordingLyrics};

use crate::{Actor, Catalog, Error, Result, plugins::Operation};
use authority::Target;
use hito_plugin_sdk::{Capability, LyricsRequest};
use std::{
    future::Future,
    time::{Duration, Instant},
};
use uuid::Uuid;

const LOOKUP_BUDGET: Duration = Duration::from_secs(10);

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "recording_lyrics", request_id = %Uuid::new_v4()))]
    pub async fn recording_lyrics_until(
        &self,
        actor: &Actor,
        recording: &str,
        request: ReadRecordingLyrics,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<LyricsLookup> {
        let started = Instant::now();
        tokio::pin!(cancelled);
        let result = tokio::select! {
            biased;
            _ = &mut cancelled => Ok(LyricsLookup::Cancelled),
            result = tokio::time::timeout(LOOKUP_BUDGET, self.read_recording_lyrics(actor, recording, request)) => {
                result.unwrap_or(Ok(LyricsLookup::Unavailable))
            }
        };
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(LyricsLookup::outcome)
                .unwrap_or_else(|error| error.code()),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Recording lyric lookup completed"
        );
        result
    }

    async fn read_recording_lyrics(
        &self,
        actor: &Actor,
        recording: &str,
        request: ReadRecordingLyrics,
    ) -> Result<LyricsLookup> {
        if request.installation_revision < 1
            || !valid_id(recording)
            || !valid_id(&request.observation_id)
        {
            return Err(Error::Invalid(
                "Use a valid recording identity and plugin revision".into(),
            ));
        }
        let target = {
            let mut tx = self.store.pool.begin().await?;
            let target =
                Target::capture(&mut tx, actor, recording, &request.observation_id).await?;
            tx.commit().await?;
            target
        };
        let operation = self
            .plugins
            .operation(
                actor,
                &target.installation_id,
                request.installation_revision,
                &[Capability::Lyrics],
            )
            .await?;
        let provider_request = LyricsRequest {
            recording: target.reference(),
        };
        let exchange = operation.lyrics_checked(&provider_request, || {
            self.recheck_recording_lyrics(actor, recording, &target, &operation)
        });
        tokio::pin!(exchange);
        let mut heartbeat = tokio::time::interval(Duration::from_millis(250));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let result = loop {
            tokio::select! {
                biased;
                _ = heartbeat.tick() => self.recheck_recording_lyrics(actor, recording, &target, &operation).await?,
                result = &mut exchange => break result,
            }
        };
        self.recheck_recording_lyrics(actor, recording, &target, &operation)
            .await?;
        match result {
            Ok(Some(document)) => Ok(LyricsLookup::Ready {
                recording_id: recording.into(),
                observation_id: request.observation_id,
                document: Box::new(document),
            }),
            Ok(None) => Ok(LyricsLookup::Unsupported),
            Err(Error::Upstream(_)) => Ok(LyricsLookup::Unavailable),
            Err(error) => Err(error),
        }
    }

    async fn recheck_recording_lyrics(
        &self,
        actor: &Actor,
        recording: &str,
        target: &Target,
        operation: &Operation,
    ) -> Result<()> {
        let mut tx = self.store.pool.begin().await?;
        target.recheck(&mut tx, actor, recording).await?;
        operation.recheck(&mut tx).await?;
        tx.commit().await?;
        Ok(())
    }
}

fn valid_id(value: &str) -> bool {
    value.len() == 36 && Uuid::parse_str(value).is_ok_and(|id| !id.is_nil())
}
