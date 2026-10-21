mod authority;
mod providers;
mod types;

pub use types::{
    LyricsLookup, ReadRecordingLyrics, RecordingLyricsProvider, RecordingLyricsProviders,
};

use crate::{Actor, Catalog, Error, Result, plugins::Operation};
use authority::Target;
use hito_plugin_sdk::{Capability, LyricsRequest};
use sha2::{Digest, Sha256};
use std::{
    future::Future,
    time::{Duration, Instant},
};
use tracing::Instrument;
use uuid::Uuid;

const LOOKUP_BUDGET: Duration = Duration::from_secs(10);

impl Catalog {
    pub async fn recording_lyrics_until(
        &self,
        actor: &Actor,
        recording: &str,
        request: ReadRecordingLyrics,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<LyricsLookup> {
        let mut completion = Completion::new();
        tokio::pin!(cancelled);
        let result = async {
            tokio::select! {
                biased;
                _ = &mut cancelled => Ok(LyricsLookup::Cancelled),
                result = tokio::time::timeout(LOOKUP_BUDGET, self.read_recording_lyrics(actor, recording, request)) => {
                    result.unwrap_or(Ok(LyricsLookup::Unavailable))
                }
            }
        }
        .instrument(completion.span.clone())
        .await;
        completion.outcome = result
            .as_ref()
            .map(LyricsLookup::outcome)
            .unwrap_or_else(|error| error.code());
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
            Ok(Some(document)) => {
                let encoded = serde_json::to_vec(&document)
                    .map_err(|_| Error::Internal("Validated lyrics could not be encoded".into()))?;
                Ok(LyricsLookup::Ready {
                    recording_id: recording.into(),
                    observation_id: request.observation_id,
                    document_digest: format!("{:x}", Sha256::digest(encoded)),
                    document: Box::new(document),
                })
            }
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

struct Completion {
    started: Instant,
    outcome: &'static str,
    span: tracing::Span,
    dispatcher: tracing::Dispatch,
    message: &'static str,
}

impl Completion {
    fn new() -> Self {
        Self::for_span(
            tracing::debug_span!("recording_lyrics_until", operation = "recording_lyrics", request_id = %Uuid::new_v4()),
            "Recording lyric lookup completed",
        )
    }

    fn providers() -> Self {
        Self::for_span(
            tracing::debug_span!("recording_lyric_providers", operation = "recording_lyric_providers", request_id = %Uuid::new_v4()),
            "Recording lyric provider list completed",
        )
    }

    fn for_span(span: tracing::Span, message: &'static str) -> Self {
        Self {
            started: Instant::now(),
            outcome: "cancelled",
            span,
            dispatcher: tracing::dispatcher::get_default(Clone::clone),
            message,
        }
    }
}

impl Drop for Completion {
    fn drop(&mut self) {
        let _dispatcher = tracing::dispatcher::set_default(&self.dispatcher);
        let span = std::mem::replace(&mut self.span, tracing::Span::none());
        let _entered = span.enter();
        tracing::debug!(
            outcome = self.outcome,
            elapsed_ms = self.started.elapsed().as_millis() as u64,
            message = self.message,
        );
    }
}
