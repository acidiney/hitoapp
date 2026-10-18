use crate::{Error, TorrentFileBinding, TorrentObservation, torrent, transport::Transport};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tracing::Instrument;
use uuid::Uuid;

const API_ROOT: &str = "https://api.real-debrid.com/rest/1.0/";
const OBSERVATION_DEADLINE: Duration = Duration::from_secs(12);

#[derive(Clone)]
pub struct RealDebridClient {
    transport: Arc<Transport>,
}

impl RealDebridClient {
    pub fn new(provider_access_token: String) -> Result<Self, Error> {
        let endpoint = API_ROOT.parse().map_err(|_| Error::Configuration)?;
        Ok(Self {
            transport: Arc::new(Transport::new(provider_access_token, endpoint)?),
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
            transport: Arc::new(Transport::new(provider_access_token, endpoint)?),
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
}
