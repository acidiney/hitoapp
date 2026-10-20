use crate::{
    Error, GeneratedFileCandidate, MediaAllowlist, OpenedBoundAudio, TorBoxClient, TorrentPhase,
    workflow::WorkflowLog,
};
use std::time::{Duration, Instant};
use tracing::Instrument;

impl TorBoxClient {
    pub async fn open_bound_audio(
        &self,
        candidate: GeneratedFileCandidate,
        allowlist: &MediaAllowlist,
    ) -> Result<OpenedBoundAudio, Error> {
        let mut log = WorkflowLog {
            started: Instant::now(),
            dispatch: tracing::dispatcher::get_default(Clone::clone),
            span: tracing::debug_span!("torbox_audio", request_id = %uuid::Uuid::new_v4(), operation = "bound_audio_opening"),
            event: "torbox_audio_opening_finished",
            outcome: "cancelled",
            count: 0,
            mint_dispatched: false,
        };
        let result = tokio::time::timeout(
            Duration::from_secs(9),
            async {
                let opening = self.media.admit()?;
                let current = self.observe(candidate.binding()).await?;
                if current.phase != TorrentPhase::ProviderCompleted {
                    return Err(Error::SourceIncomplete);
                }
                opening
                    .open(
                        candidate.private_link(),
                        current.binding.file_bytes,
                        current.binding,
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
