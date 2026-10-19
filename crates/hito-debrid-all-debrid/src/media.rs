use crate::{
    AllDebridClient, Error, GeneratedFileCandidate, MagnetPhase, MediaAllowlist, OpenedBoundAudio,
    client::OperationLog,
};
use std::time::{Duration, Instant};
use tracing::Instrument;

impl AllDebridClient {
    pub async fn open_bound_audio(
        &self,
        candidate: GeneratedFileCandidate,
        allowlist: &MediaAllowlist,
    ) -> Result<OpenedBoundAudio, Error> {
        let mut opening_log = OperationLog {
            started: Instant::now(),
            span: tracing::debug_span!("all_debrid_audio", request_id = %uuid::Uuid::new_v4(), operation = "bound_audio_opening"),
            event: "all_debrid_audio_opening_finished",
            outcome: "cancelled",
            count: 0,
            post_dispatched: false,
            unknown_outcome: "cancelled",
        };
        let result = tokio::time::timeout(
            Duration::from_secs(9),
            async {
                let opening = self.media.admit()?;
                let observation = self.observe(candidate.binding()).await?;
                if observation.phase != MagnetPhase::ProviderCompleted {
                    return Err(Error::UnsupportedState);
                }
                opening
                    .open(
                        candidate.private_link(),
                        observation.binding.file_bytes,
                        observation.binding,
                        allowlist,
                    )
                    .await
                    .map_err(Error::from)
            }
            .instrument(opening_log.span.clone()),
        )
        .await
        .unwrap_or(Err(Error::Deadline));
        opening_log.outcome = result
            .as_ref()
            .map(|_| "complete")
            .unwrap_or_else(|error| error.outcome());
        opening_log.count = u64::from(result.is_ok());
        result
    }
}
