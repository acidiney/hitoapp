use crate::{Error, PremiumizeClient, TransferFileBinding, TransferPhase, TransferProgress, file};
use std::time::{Duration, Instant};
use tracing::Instrument;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoundFilePhase {
    ProviderSeeding,
    ProviderCompleted,
}

pub struct BoundFileObservation {
    pub binding: TransferFileBinding,
    pub phase: BoundFilePhase,
    pub progress: TransferProgress,
}

impl PremiumizeClient {
    pub async fn observe_bound_file(
        &self,
        binding: &TransferFileBinding,
    ) -> Result<BoundFileObservation, Error> {
        let mut log = Completion {
            started: Instant::now(),
            dispatch: tracing::dispatcher::get_default(Clone::clone),
            span: tracing::debug_span!("premiumize_file_observation", request_id = %uuid::Uuid::new_v4(), operation = "bound_file_observation"),
            outcome: "cancelled",
            count: 0,
        };
        let result = tokio::time::timeout(
            Duration::from_secs(9),
            async {
                binding.validate()?;
                let _permit = self.transport.admit()?;
                let bytes = self.transport.observe().await?;
                let (phase, progress) = file::validate_transfer(&bytes, binding)?;
                let phase = match phase {
                    TransferPhase::ProviderSeeding => BoundFilePhase::ProviderSeeding,
                    TransferPhase::ProviderCompleted => BoundFilePhase::ProviderCompleted,
                    _ => return Err(Error::UnsupportedState),
                };
                let bytes = self.transport.item_details(&binding.item_id).await?;
                drop(file::bound_link(&bytes, binding)?);
                Ok(BoundFileObservation {
                    binding: binding.clone(),
                    phase,
                    progress,
                })
            }
            .instrument(log.span.clone()),
        )
        .await
        .unwrap_or(Err(Error::Deadline));
        log.outcome = match &result {
            Ok(observed) => match observed.phase {
                BoundFilePhase::ProviderSeeding => "provider_seeding",
                BoundFilePhase::ProviderCompleted => "provider_completed",
            },
            Err(error) => error.outcome(),
        };
        log.count = u64::from(result.is_ok());
        result
    }
}

struct Completion {
    started: Instant,
    dispatch: tracing::Dispatch,
    span: tracing::Span,
    outcome: &'static str,
    count: u64,
}

impl Drop for Completion {
    fn drop(&mut self) {
        let _dispatch = tracing::dispatcher::set_default(&self.dispatch);
        let span = std::mem::replace(&mut self.span, tracing::Span::none());
        let _entered = span.enter();
        tracing::debug!(
            event = "premiumize_file_observation_finished",
            outcome = self.outcome,
            count = self.count,
            elapsed_ms = self.started.elapsed().as_millis() as u64,
        );
    }
}
