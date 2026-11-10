use super::{PcmStream, unfinished};
use crate::{Error, PlaybackSource, Result};
use std::time::Duration;
use tokio::{sync::watch, task::JoinHandle};
use tracing::{Instrument, instrument::WithSubscriber};

#[derive(Clone)]
pub(super) struct Stop(watch::Sender<bool>);

impl Stop {
    fn new() -> Self {
        let (sender, _) = watch::channel(false);
        Self(sender)
    }

    pub(super) fn cancel(&self) {
        self.0.send_replace(true);
    }

    pub(super) fn check(&self) -> Result<()> {
        if *self.0.borrow() {
            Err(cancelled())
        } else {
            Ok(())
        }
    }

    pub(super) async fn cancelled(&self) {
        let mut receiver = self.0.subscribe();
        let _ = receiver.wait_for(|cancelled| *cancelled).await;
    }
}

pub struct PcmOpening {
    grant_id: u64,
    stop: Stop,
    owner: Option<JoinHandle<Result<PcmStream>>>,
}

impl PlaybackSource {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(playback_grant = self.correlation_id(), account_id = self.actor().id()))]
    pub fn begin_pcm(&self, start: Duration) -> Result<PcmOpening> {
        if start > super::super::MAX_CONVERSION_START {
            return Err(Error::Invalid("Start position is out of range".into()));
        }
        let runtime = tokio::runtime::Handle::try_current().map_err(|_| unfinished())?;
        let stop = Stop::new();
        let owner_stop = stop.clone();
        let authority = self.clone();
        let span = tracing::debug_span!(
            "native_pcm_opening",
            playback_grant = self.correlation_id(),
            account_id = self.actor().id()
        );
        let owner = runtime.spawn(
            async move {
                let result = authority.decode_pcm(start, owner_stop.clone()).await;
                if let Err(error) = &result {
                    let outcome = if owner_stop.check().is_err() {
                        "cancelled"
                    } else {
                        "failed"
                    };
                    tracing::debug!(
                        event = "native_pcm_open_failed",
                        outcome,
                        error_kind = error.code(),
                        "Native decoder did not open"
                    );
                }
                result
            }
            .instrument(span)
            .with_current_subscriber(),
        );
        Ok(PcmOpening {
            grant_id: self.correlation_id(),
            stop,
            owner: Some(owner),
        })
    }
}

impl PcmOpening {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(playback_grant = self.grant_id))]
    pub async fn ready(&mut self) -> Result<PcmStream> {
        let owner = self.owner.as_mut().ok_or_else(unfinished)?;
        let result = owner.await;
        self.owner = None;
        result.map_err(|_| unfinished())?
    }

    #[tracing::instrument(level = "debug", skip_all, fields(playback_grant = self.grant_id))]
    pub async fn close(mut self) -> Result<()> {
        if let Some(owner) = self.owner.take() {
            self.stop.cancel();
            if let Ok(stream) = owner.await.map_err(|_| unfinished())? {
                stream.close().await?;
            }
        }
        Ok(())
    }
}

impl Drop for PcmOpening {
    fn drop(&mut self) {
        if self.owner.is_some() {
            self.stop.cancel();
        }
    }
}

pub(super) fn cancelled() -> Error {
    Error::Upstream("Native audio decoding was cancelled")
}
