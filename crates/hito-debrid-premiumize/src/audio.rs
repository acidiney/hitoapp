use crate::{Error, PremiumizeClient, TransferBinding, file};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tracing::Instrument;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferFileBinding {
    pub transfer: TransferBinding,
    pub item_id: String,
    pub item_created_at_unix_seconds: u64,
    pub file_name: String,
    pub file_bytes: u64,
    pub folder_id: String,
}

impl TransferFileBinding {
    pub fn validate(&self) -> Result<(), Error> {
        self.transfer.validate()?;
        if [&self.item_id, &self.folder_id].iter().any(|value| {
            value.is_empty()
                || value.len() > 256
                || value.trim() != value.as_str()
                || value.chars().any(char::is_control)
        }) || self.file_name.is_empty()
            || self.file_name.len() > 4096
            || self.file_name.chars().any(char::is_control)
            || self.file_bytes < 44
        {
            return Err(Error::InvalidBinding);
        }
        Ok(())
    }
}

pub type MediaAllowlist = hito_remote_audio::MediaAllowlist<Error>;
pub type OpenedBoundAudio = hito_remote_audio::OpenedBoundAudio<TransferFileBinding, Error>;

impl PremiumizeClient {
    pub async fn open_bound_audio(
        &self,
        binding: &TransferFileBinding,
        allowlist: &MediaAllowlist,
    ) -> Result<OpenedBoundAudio, Error> {
        let mut log = AudioLog {
            started: Instant::now(),
            dispatch: tracing::dispatcher::get_default(Clone::clone),
            span: tracing::debug_span!("premiumize_audio", request_id = %uuid::Uuid::new_v4(), operation = "bound_audio_opening"),
            outcome: "cancelled",
            count: 0,
        };
        let result = tokio::time::timeout(
            Duration::from_secs(9),
            async {
                binding.validate()?;
                let opening = self.media.admit()?;
                let permit = self.transport.admit()?;
                let bytes = self.transport.observe().await?;
                file::validate_transfer(&bytes, binding)?;
                let bytes = self.transport.item_details(&binding.item_id).await?;
                let link = file::bound_link(&bytes, binding)?;
                drop(permit);
                opening
                    .open(
                        link.as_str(),
                        binding.file_bytes,
                        binding.clone(),
                        allowlist,
                    )
                    .await
                    .map_err(Error::from)
            }
            .instrument(log.span.clone()),
        )
        .await
        .unwrap_or(Err(Error::Deadline));
        log.outcome = result
            .as_ref()
            .map(|_| "complete")
            .unwrap_or_else(|error| error.outcome());
        log.count = u64::from(result.is_ok());
        result
    }
}

struct AudioLog {
    started: Instant,
    dispatch: tracing::Dispatch,
    span: tracing::Span,
    outcome: &'static str,
    count: u64,
}

impl Drop for AudioLog {
    fn drop(&mut self) {
        let _dispatch = tracing::dispatcher::set_default(&self.dispatch);
        let span = std::mem::replace(&mut self.span, tracing::Span::none());
        let _entered = span.enter();
        tracing::debug!(
            event = "premiumize_audio_opening_finished",
            outcome = self.outcome,
            count = self.count,
            elapsed_ms = self.started.elapsed().as_millis() as u64,
        );
    }
}
