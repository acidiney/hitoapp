use crate::{
    DelayedFileReceipt, DelayedPoll, Error, MagnetFileBinding, MagnetFileSelection,
    MagnetObservation, MagnetPhase, MagnetUploadReceipt, delayed, magnet, prepare, submit,
    transport::{Resource, Transport},
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct AllDebridClient {
    transport: Arc<Transport>,
}

impl AllDebridClient {
    pub fn new(provider_api_key: String) -> Result<Self, Error> {
        Ok(Self {
            transport: Arc::new(Transport::new(provider_api_key)?),
        })
    }

    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn with_test_tls(
        provider_api_key: String,
        certificate_der: &[u8],
        api_address: std::net::SocketAddr,
    ) -> Result<Self, Error> {
        Ok(Self {
            transport: Arc::new(Transport::with_test_tls(
                provider_api_key,
                certificate_der,
                api_address,
            )?),
        })
    }

    pub async fn observe(&self, binding: &MagnetFileBinding) -> Result<MagnetObservation, Error> {
        let mut observation = OperationLog {
            started: Instant::now(),
            span: tracing::debug_span!("all_debrid_observation", request_id = %uuid::Uuid::new_v4(), operation = "magnet_observation"),
            event: "all_debrid_observation_finished",
            outcome: "cancelled",
            count: 0,
            post_dispatched: false,
            unknown_outcome: "cancelled",
        };
        let result = tokio::time::timeout(Duration::from_secs(12), async {
            binding.validate()?;
            let _permit = self.transport.admit()?;
            let bytes = self
                .transport
                .read(Resource::Status, binding.magnet_id)
                .await?;
            let (phase, progress_basis_points) = magnet::status(&bytes, binding)?;
            if phase == MagnetPhase::ProviderCompleted {
                let bytes = self
                    .transport
                    .read(Resource::Files, binding.magnet_id)
                    .await?;
                magnet::bound_file_link(&bytes, binding)?;
            }
            Ok(MagnetObservation {
                binding: binding.clone(),
                phase,
                progress_basis_points,
            })
        })
        .await
        .unwrap_or(Err(Error::Deadline));
        observation.outcome = match &result {
            Ok(value) => match value.phase {
                MagnetPhase::Processing => "processing",
                MagnetPhase::ProviderCompleted => "provider_completed",
            },
            Err(error) => error.outcome(),
        };
        observation.count = u64::from(result.is_ok());
        result
    }

    pub async fn submit_magnet(
        &self,
        selection: &MagnetFileSelection,
    ) -> Result<MagnetUploadReceipt, Error> {
        let mut submission = OperationLog {
            started: Instant::now(),
            span: tracing::debug_span!("all_debrid_submission", request_id = %uuid::Uuid::new_v4(), operation = "magnet_submission"),
            event: "all_debrid_submission_finished",
            outcome: "cancelled",
            count: 0,
            post_dispatched: false,
            unknown_outcome: "submission_unknown",
        };
        let result = tokio::time::timeout(Duration::from_secs(12), async {
            selection.validate()?;
            let _permit = self.transport.admit()?;
            let bytes = self
                .transport
                .upload(&selection.info_hash, &mut submission.post_dispatched)
                .await?;
            submit::receipt(&bytes, selection)
        })
        .await
        .unwrap_or(Err(Error::Deadline));
        let result = if submission.post_dispatched {
            result.map_err(|error| match error {
                Error::Status(401 | 403) => error,
                _ => Error::SubmissionUnknown,
            })
        } else {
            result
        };
        submission.outcome = result
            .as_ref()
            .map(|_| "acknowledged")
            .unwrap_or_else(|error| error.outcome());
        submission.count = u64::from(result.is_ok());
        result
    }

    pub async fn poll_delayed(
        &self,
        receipt: &mut DelayedFileReceipt,
    ) -> Result<DelayedPoll, Error> {
        let mut polling = OperationLog {
            started: Instant::now(),
            span: tracing::debug_span!("all_debrid_delayed_poll", request_id = %uuid::Uuid::new_v4(), operation = "delayed_file_poll"),
            event: "all_debrid_delayed_poll_finished",
            outcome: "cancelled",
            count: 0,
            post_dispatched: false,
            unknown_outcome: "cancelled",
        };
        let result = tokio::time::timeout(Duration::from_secs(12), async {
            let _permit = self.transport.admit()?;
            let attempt = receipt.attempt();
            attempt.wait().await;
            let bytes = self
                .transport
                .read(Resource::Delayed, attempt.delayed_id)
                .await?;
            delayed::observation(&bytes, attempt.binding)
        })
        .await
        .unwrap_or(Err(Error::Deadline));
        polling.outcome = match &result {
            Ok(DelayedPoll::Processing { .. }) => "processing",
            Ok(DelayedPoll::Candidate(_)) => "candidate",
            Err(error) => error.outcome(),
        };
        polling.count = u64::from(result.is_ok());
        result
    }

    /// Requests one unlock after re-observation. Only delayed receipts are supported.
    /// Generated links remain unsupported; this operation never polls or fetches media.
    pub async fn prepare_bound_file(
        &self,
        binding: &MagnetFileBinding,
    ) -> Result<DelayedFileReceipt, Error> {
        let mut preparation = OperationLog {
            started: Instant::now(),
            span: tracing::debug_span!("all_debrid_preparation", request_id = %uuid::Uuid::new_v4(), operation = "bound_file_preparation"),
            event: "all_debrid_preparation_finished",
            outcome: "cancelled",
            count: 0,
            post_dispatched: false,
            unknown_outcome: "unlock_unknown",
        };
        let mut acknowledged = false;
        let result = tokio::time::timeout(Duration::from_secs(12), async {
            binding.validate()?;
            let _permit = self.transport.admit()?;
            let bytes = self
                .transport
                .read(Resource::Status, binding.magnet_id)
                .await?;
            let (phase, _) = magnet::status(&bytes, binding)?;
            if phase != MagnetPhase::ProviderCompleted {
                return Err(Error::UnsupportedState);
            }
            let bytes = self
                .transport
                .read(Resource::Files, binding.magnet_id)
                .await?;
            let link = magnet::bound_file_link(&bytes, binding)?;
            let bytes = self
                .transport
                .unlock(&link, &mut preparation.post_dispatched)
                .await?;
            let receipt = prepare::acknowledgment(&bytes, binding)?;
            acknowledged = true;
            match receipt {
                prepare::Acknowledgment::Delayed(receipt) => Ok(receipt),
                prepare::Acknowledgment::GeneratedUnsupported => {
                    Err(Error::GeneratedLinkUnsupported)
                }
            }
        })
        .await
        .unwrap_or(Err(Error::Deadline));
        let result = if preparation.post_dispatched && !acknowledged {
            result.map_err(|error| match error {
                Error::Status(401 | 403) => error,
                _ => Error::UnlockUnknown,
            })
        } else {
            result
        };
        preparation.outcome = result
            .as_ref()
            .map(|_| "delayed")
            .unwrap_or_else(|error| error.outcome());
        preparation.count = u64::from(result.is_ok());
        result
    }
}

struct OperationLog {
    started: Instant,
    span: tracing::Span,
    event: &'static str,
    outcome: &'static str,
    count: u64,
    post_dispatched: bool,
    unknown_outcome: &'static str,
}

impl Drop for OperationLog {
    fn drop(&mut self) {
        let _entered = self.span.enter();
        let outcome = if self.outcome == "cancelled" && self.post_dispatched {
            self.unknown_outcome
        } else {
            self.outcome
        };
        tracing::debug!(
            event = self.event,
            outcome,
            count = self.count,
            elapsed_ms = self.started.elapsed().as_millis() as u64,
        );
    }
}
