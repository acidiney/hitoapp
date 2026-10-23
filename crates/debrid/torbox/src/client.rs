use crate::{Error, TorrentFileBinding, observation, transport::Transport};
use hito_remote_audio::{MediaClient, network::Dns};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TorrentPhase {
    Incomplete,
    ProviderCompleted,
}

pub struct TorrentObservation {
    pub binding: TorrentFileBinding,
    pub phase: TorrentPhase,
}

#[derive(Clone)]
pub struct TorBoxClient {
    pub(crate) transport: Arc<Transport>,
    pub(crate) media: Arc<MediaClient>,
}

impl TorBoxClient {
    pub fn new(provider_api_token: String) -> Result<Self, Error> {
        Ok(Self {
            transport: Arc::new(Transport::new(provider_api_token)?),
            media: Arc::new(MediaClient::new(
                Arc::new(Dns::system().map_err(|_| Error::Configuration)?),
                concat!("HitoTorBox/", env!("CARGO_PKG_VERSION")),
            )),
        })
    }

    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn with_test_tls(
        provider_api_token: String,
        certificate_der: &[u8],
        api_address: std::net::SocketAddr,
    ) -> Result<Self, Error> {
        Ok(Self {
            transport: Arc::new(Transport::with_test_tls(
                provider_api_token,
                certificate_der,
                api_address,
            )?),
            media: Arc::new(MediaClient::for_api_fixture(concat!(
                "HitoTorBox/",
                env!("CARGO_PKG_VERSION")
            ))),
        })
    }

    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn with_test_tls_media(
        provider_api_token: String,
        api_certificate_der: &[u8],
        api_address: std::net::SocketAddr,
        media_certificate_der: &[u8],
        media_address: std::net::SocketAddr,
    ) -> Result<Self, Error> {
        Ok(Self {
            transport: Arc::new(Transport::with_test_tls(
                provider_api_token,
                api_certificate_der,
                api_address,
            )?),
            media: Arc::new(MediaClient::with_test_tls(
                media_certificate_der,
                "media.hito-fixture.invalid",
                media_address,
                concat!("HitoTorBox/", env!("CARGO_PKG_VERSION")),
            )?),
        })
    }

    pub async fn observe(&self, binding: &TorrentFileBinding) -> Result<TorrentObservation, Error> {
        let mut log = ObservationLog {
            started: Instant::now(),
            dispatch: tracing::dispatcher::get_default(Clone::clone),
            span: tracing::debug_span!("torbox_observation", request_id = %uuid::Uuid::new_v4(), operation = "torrent_observation"),
            outcome: "cancelled",
            count: 0,
        };
        let result = tokio::time::timeout(Duration::from_secs(12), async {
            binding.validate()?;
            let _permit = self.transport.admit()?;
            let bytes = self.transport.observe(binding.torrent_id).await?;
            let phase = observation::phase(&bytes, binding)?;
            Ok(TorrentObservation {
                binding: binding.clone(),
                phase,
            })
        })
        .await
        .unwrap_or(Err(Error::Deadline));
        log.outcome = match &result {
            Ok(value) => match value.phase {
                TorrentPhase::Incomplete => "incomplete",
                TorrentPhase::ProviderCompleted => "provider_completed",
            },
            Err(error) => error.outcome(),
        };
        log.count = u64::from(result.is_ok());
        result
    }
}

struct ObservationLog {
    started: Instant,
    dispatch: tracing::Dispatch,
    span: tracing::Span,
    outcome: &'static str,
    count: u64,
}

impl Drop for ObservationLog {
    fn drop(&mut self) {
        let _dispatch = tracing::dispatcher::set_default(&self.dispatch);
        let span = std::mem::replace(&mut self.span, tracing::Span::none());
        let _entered = span.enter();
        tracing::debug!(
            event = "torbox_observation_finished",
            outcome = self.outcome,
            count = self.count,
            elapsed_ms = self.started.elapsed().as_millis() as u64,
        );
    }
}
