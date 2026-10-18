use crate::{
    Error, MediaAllowlist, OpenedBoundAudio, TorrentFileBinding, TorrentObservation,
    UnrestrictionReceipt,
    audio::Media,
    network::Dns,
    torrent,
    transport::{self, Transport},
    unrestriction,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::Instrument;
use uuid::Uuid;

const API_ROOT: &str = "https://api.real-debrid.com/rest/1.0/";
const OBSERVATION_DEADLINE: Duration = Duration::from_secs(12);
const UNRESTRICTION_DEADLINE: Duration = Duration::from_secs(12);
const AUDIO_OPENING_DEADLINE: Duration = Duration::from_secs(9);

#[derive(Clone)]
pub struct RealDebridClient {
    transport: Arc<Transport>,
    media: Arc<Media>,
}

impl RealDebridClient {
    pub fn new(provider_access_token: String) -> Result<Self, Error> {
        let endpoint = API_ROOT.parse().map_err(|_| Error::Configuration)?;
        let authorization = transport::authorization(provider_access_token)?;
        let dns = Arc::new(Dns::system().map_err(|_| Error::Configuration)?);
        Ok(Self {
            transport: Arc::new(Transport::new(authorization, endpoint, dns.clone())?),
            media: Arc::new(Media::new(dns)),
        })
    }

    #[cfg(test)]
    pub(crate) fn with_test_endpoint(
        provider_access_token: String,
        endpoint: reqwest::Url,
    ) -> Result<Self, Error> {
        let loopback = endpoint
            .host_str()
            .and_then(|host| host.parse::<std::net::IpAddr>().ok())
            .is_some_and(|address| address.is_loopback());
        if endpoint.scheme() != "http"
            || !loopback
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || endpoint.path() != "/rest/1.0/"
        {
            return Err(Error::Configuration);
        }
        Ok(Self {
            transport: Arc::new(Transport::with_test_endpoint(
                provider_access_token,
                endpoint,
            )?),
            media: Arc::new(Media::for_api_fixture()),
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_test_tls(
        provider_access_token: String,
        ca_der: &[u8],
        api_address: std::net::SocketAddr,
        media: (&str, std::net::SocketAddr),
    ) -> Result<Self, Error> {
        let endpoint = API_ROOT.parse().map_err(|_| Error::Configuration)?;
        Ok(Self {
            transport: Arc::new(Transport::with_test_tls(
                provider_access_token,
                endpoint,
                ca_der,
                api_address,
            )?),
            media: Arc::new(Media::with_test_tls(ca_der, media.0, media.1)?),
        })
    }

    #[cfg(test)]
    pub(crate) fn with_test_tls_dns(
        provider_access_token: String,
        ca_der: &[u8],
        api_address: std::net::SocketAddr,
        media: (&str, std::net::SocketAddr),
        dns_address: std::net::SocketAddr,
    ) -> Result<Self, Error> {
        let endpoint = API_ROOT.parse().map_err(|_| Error::Configuration)?;
        Ok(Self {
            transport: Arc::new(Transport::with_test_tls(
                provider_access_token,
                endpoint,
                ca_der,
                api_address,
            )?),
            media: Arc::new(Media::with_test_tls_dns(
                ca_der,
                media.0,
                media.1,
                dns_address,
            )?),
        })
    }

    #[cfg(test)]
    pub(crate) fn with_test_api_dns(
        provider_access_token: String,
        ca_der: &[u8],
        api_address: std::net::SocketAddr,
        dns_address: std::net::SocketAddr,
    ) -> Result<Self, Error> {
        let endpoint = API_ROOT.parse().map_err(|_| Error::Configuration)?;
        Ok(Self {
            transport: Arc::new(Transport::with_test_api_dns(
                provider_access_token,
                endpoint,
                ca_der,
                api_address,
                dns_address,
            )?),
            media: Arc::new(Media::for_api_fixture()),
        })
    }

    pub async fn observe(&self, binding: &TorrentFileBinding) -> Result<TorrentObservation, Error> {
        let started = Instant::now();
        let request_id = Uuid::new_v4();
        let span = tracing::debug_span!(
            "real_debrid_observation", %request_id, operation = "torrent_observation"
        );
        async {
            let result = tokio::time::timeout(OBSERVATION_DEADLINE, async {
                binding.validate()?;
                let _permit = self.transport.admit()?;
                let bytes = self.transport.torrent_info(&binding.torrent_id).await?;
                torrent::observation(&bytes, binding)
            })
            .await
            .unwrap_or(Err(Error::Deadline));
            tracing::debug!(
                event = "real_debrid_observation_finished",
                outcome = result
                    .as_ref()
                    .map(|_| "success")
                    .unwrap_or_else(|error| error.outcome()),
                count = if result.is_ok() { 1_u64 } else { 0 },
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Real-Debrid torrent observation finished"
            );
            result
        }
        .instrument(span)
        .await
    }

    pub async fn unrestrict_bound_file(
        &self,
        binding: &TorrentFileBinding,
    ) -> Result<UnrestrictionReceipt, Error> {
        let started = Instant::now();
        let request_id = Uuid::new_v4();
        let span = tracing::debug_span!(
            "real_debrid_unrestriction", %request_id, operation = "file_unrestriction"
        );
        async {
            let mut post_dispatched = false;
            let result = tokio::time::timeout(UNRESTRICTION_DEADLINE, async {
                binding.validate()?;
                self.resolve_candidate(binding, &mut post_dispatched)
                    .await
                    .map(|candidate| candidate.into_receipt())
            })
            .await
            .unwrap_or(Err(Error::Deadline));
            let result = if post_dispatched {
                result.map_err(|error| match error {
                    Error::Status(401 | 403) => error,
                    _ => Error::UnrestrictionUncertain,
                })
            } else {
                result
            };
            tracing::debug!(
                event = "real_debrid_unrestriction_finished",
                outcome = result
                    .as_ref()
                    .map(|_| "success")
                    .unwrap_or_else(|error| error.outcome()),
                count = if result.is_ok() { 1_u64 } else { 0 },
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Real-Debrid file unrestriction finished"
            );
            result
        }
        .instrument(span)
        .await
    }

    pub async fn open_bound_audio(
        &self,
        binding: &TorrentFileBinding,
        allowlist: &MediaAllowlist,
    ) -> Result<OpenedBoundAudio, Error> {
        let started = Instant::now();
        let request_id = Uuid::new_v4();
        let span = tracing::debug_span!(
            "real_debrid_audio", %request_id, operation = "bound_audio_opening"
        );
        async {
            let mut post_dispatched = false;
            let mut acknowledged = false;
            let result = tokio::time::timeout(AUDIO_OPENING_DEADLINE, async {
                binding.validate()?;
                let permit = self.media.admit()?;
                let candidate = self
                    .resolve_candidate(binding, &mut post_dispatched)
                    .await?;
                acknowledged = true;
                self.media.open(candidate, allowlist, permit).await
            })
            .await
            .unwrap_or(Err(Error::Deadline));
            let result = if post_dispatched && !acknowledged {
                result.map_err(|error| match error {
                    Error::Status(401 | 403) => error,
                    _ => Error::UnrestrictionUncertain,
                })
            } else {
                result
            };
            tracing::debug!(
                event = "real_debrid_audio_opening_finished",
                outcome = result
                    .as_ref()
                    .map(|_| "complete")
                    .unwrap_or_else(|error| error.outcome()),
                count = if result.is_ok() { 1_u64 } else { 0 },
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Real-Debrid bound audio opening finished"
            );
            result
        }
        .instrument(span)
        .await
    }

    async fn resolve_candidate(
        &self,
        binding: &TorrentFileBinding,
        post_dispatched: &mut bool,
    ) -> Result<unrestriction::ValidatedCandidate, Error> {
        let _permit = self.transport.admit()?;
        let bytes = self.transport.torrent_info(&binding.torrent_id).await?;
        let link = match torrent::bound_torrent(&bytes, binding)? {
            torrent::BoundTorrent::ProviderCompleted { link } => link,
            torrent::BoundTorrent::Downloading { .. } => return Err(Error::UnsupportedState),
        };
        let bytes = self
            .transport
            .unrestrict_link(&link, post_dispatched)
            .await?;
        unrestriction::candidate(&bytes, binding, &link)
    }
}
