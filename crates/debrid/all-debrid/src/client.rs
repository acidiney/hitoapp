use crate::{
    DelayedFileReceipt, DelayedPoll, Error, MagnetFileBinding, MagnetFileSelection,
    MagnetObservation, MagnetPhase, MagnetUploadReceipt, PreparedFile, delayed, magnet, prepare,
    submit,
    transport::{Resource, Transport},
};
use hito_remote_audio::{MediaClient, network::Dns};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

const MEDIA_AGENT: &str = concat!("HitoAllDebrid/", env!("CARGO_PKG_VERSION"));

#[derive(Clone)]
pub struct AllDebridClient {
    transport: Arc<Transport>,
    pub(crate) media: Arc<MediaClient>,
}

impl AllDebridClient {
    pub fn new(provider_api_key: String) -> Result<Self, Error> {
        Ok(Self {
            transport: Arc::new(Transport::new(provider_api_key)?),
            media: Arc::new(MediaClient::new(
                Arc::new(Dns::system().map_err(|_| Error::Configuration)?),
                MEDIA_AGENT,
            )),
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
            media: Arc::new(MediaClient::for_api_fixture(MEDIA_AGENT)),
        })
    }

    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn with_test_tls_media(
        provider_api_key: String,
        api_certificate_der: &[u8],
        api_address: std::net::SocketAddr,
        media_certificate_der: &[u8],
        media_address: std::net::SocketAddr,
    ) -> Result<Self, Error> {
        Ok(Self {
            transport: Arc::new(Transport::with_test_tls(
                provider_api_key,
                api_certificate_der,
                api_address,
            )?),
            media: Arc::new(MediaClient::with_test_tls(
                media_certificate_der,
                "media.hito-fixture.invalid",
                media_address,
                MEDIA_AGENT,
            )?),
        })
    }

    pub async fn observe(&self, binding: &MagnetFileBinding) -> Result<MagnetObservation, Error> {
        let mut observation = OperationLog {
            started: Instant::now(),
            dispatch: tracing::dispatcher::get_default(Clone::clone),
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
            dispatch: tracing::dispatcher::get_default(Clone::clone),
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
            dispatch: tracing::dispatcher::get_default(Clone::clone),
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

    pub async fn prepare_bound_file(
        &self,
        binding: &MagnetFileBinding,
    ) -> Result<PreparedFile, Error> {
        let mut preparation = OperationLog {
            started: Instant::now(),
            dispatch: tracing::dispatcher::get_default(Clone::clone),
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
            Ok(receipt)
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
        preparation.outcome = match &result {
            Ok(PreparedFile::Delayed(_)) => "delayed",
            Ok(PreparedFile::Candidate(_)) => "candidate",
            Err(error) => error.outcome(),
        };
        preparation.count = u64::from(result.is_ok());
        result
    }
}

pub(crate) struct OperationLog {
    pub started: Instant,
    pub dispatch: tracing::Dispatch,
    pub span: tracing::Span,
    pub event: &'static str,
    pub outcome: &'static str,
    pub count: u64,
    pub post_dispatched: bool,
    pub unknown_outcome: &'static str,
}

impl Drop for OperationLog {
    fn drop(&mut self) {
        let _dispatch = tracing::dispatcher::set_default(&self.dispatch);
        let span = std::mem::replace(&mut self.span, tracing::Span::none());
        let _entered = span.enter();
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
