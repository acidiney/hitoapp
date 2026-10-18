use super::{MediaAllowlist, OpenedBoundAudio};
use crate::{
    Error,
    network::{self, Dns, DnsPurpose, NetworkError, PinnedResolver},
    unrestriction::ValidatedCandidate,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub(crate) struct Media {
    slots: Arc<Semaphore>,
    dns: Option<Arc<Dns>>,
    #[cfg(test)]
    tls: Option<TestTls>,
}

#[cfg(test)]
struct TestTls {
    certificate: reqwest::Certificate,
    address: std::net::SocketAddr,
    host: &'static str,
}

impl Media {
    pub(crate) fn new(dns: Arc<Dns>) -> Self {
        Self {
            slots: Arc::new(Semaphore::new(2)),
            dns: Some(dns),
            #[cfg(test)]
            tls: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_api_fixture() -> Self {
        Self {
            slots: Arc::new(Semaphore::new(2)),
            dns: None,
            tls: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_test_tls(
        ca_der: &[u8],
        host: &str,
        address: std::net::SocketAddr,
    ) -> Result<Self, Error> {
        if host != "media.hito-fixture.invalid" {
            return Err(Error::Configuration);
        }
        Self::test_tls(ca_der, "media.hito-fixture.invalid", address, None)
    }

    #[cfg(test)]
    fn test_tls(
        ca_der: &[u8],
        host: &'static str,
        address: std::net::SocketAddr,
        dns: Option<Arc<Dns>>,
    ) -> Result<Self, Error> {
        if !address.ip().is_loopback() || address.port() == 0 || ca_der.len() > 16_384 {
            return Err(Error::Configuration);
        }
        let certificate =
            reqwest::Certificate::from_der(ca_der).map_err(|_| Error::Configuration)?;
        Ok(Self {
            slots: Arc::new(Semaphore::new(2)),
            dns,
            tls: Some(TestTls {
                certificate,
                address,
                host,
            }),
        })
    }

    #[cfg(test)]
    pub(crate) fn with_test_tls_dns(
        ca_der: &[u8],
        host: &str,
        address: std::net::SocketAddr,
        dns_address: std::net::SocketAddr,
    ) -> Result<Self, Error> {
        if host != "media.hito-fixture.test" {
            return Err(Error::Configuration);
        }
        Self::test_tls(
            ca_der,
            "media.hito-fixture.test",
            address,
            Some(Arc::new(
                Dns::with_test_tcp(dns_address).map_err(network_error)?,
            )),
        )
    }

    pub(crate) fn admit(&self) -> Result<OwnedSemaphorePermit, Error> {
        self.slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)
    }

    pub(crate) async fn open(
        &self,
        candidate: ValidatedCandidate,
        allowlist: &MediaAllowlist,
        permit: OwnedSemaphorePermit,
    ) -> Result<OpenedBoundAudio, Error> {
        let target = allowlist.target(&candidate.download)?;
        let host = target.host_str().ok_or(Error::MediaDestination)?;
        let addresses = self.addresses(host).await?;
        let builder = reqwest::Client::builder()
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .referer(false)
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .http1_only()
            .http1_max_headers(32)
            .pool_max_idle_per_host(0)
            .connect_timeout(Duration::from_secs(2))
            .user_agent(concat!("HitoRealDebrid/", env!("CARGO_PKG_VERSION")))
            .dns_resolver(Arc::new(PinnedResolver::new(
                host.to_owned(),
                addresses.clone(),
            )));
        #[cfg(test)]
        let builder = match &self.tls {
            Some(tls) => builder.tls_certs_only([tls.certificate.clone()]),
            None => builder,
        };
        let client = builder.build().map_err(|_| Error::Configuration)?;
        let response = client
            .get(target)
            .header("accept", "audio/wav")
            .header("accept-encoding", "identity")
            .header("connection", "close")
            .send()
            .await
            .map_err(|_| Error::MediaTransport)?;
        if response
            .remote_addr()
            .is_none_or(|peer| !addresses.contains(&peer))
        {
            return Err(Error::MediaDestination);
        }
        validate_response(&response, candidate.confirmed_bytes)?;
        OpenedBoundAudio::qualify(
            response,
            candidate.binding,
            candidate.confirmed_bytes,
            permit,
        )
        .await
    }

    async fn addresses(&self, host: &str) -> Result<Vec<std::net::SocketAddr>, Error> {
        #[cfg(test)]
        if let Some(tls) = &self.tls {
            if host != tls.host {
                return Err(Error::MediaDestination);
            }
            if self.dns.is_none() {
                return Ok(vec![tls.address]);
            }
        }
        let dns = self.dns.as_ref().ok_or(Error::Configuration)?;
        let resolved = dns
            .addresses(host, DnsPurpose::Media)
            .await
            .map_err(network_error)?;
        let addresses = network::public_addresses(resolved).map_err(network_error)?;
        #[cfg(test)]
        if let Some(tls) = &self.tls {
            return Ok(vec![tls.address]);
        }
        Ok(addresses)
    }
}

fn network_error(error: NetworkError) -> Error {
    match error {
        NetworkError::Configuration => Error::Configuration,
        NetworkError::Transport => Error::MediaTransport,
        NetworkError::Destination => Error::MediaDestination,
    }
}

fn validate_response(response: &reqwest::Response, length: u64) -> Result<(), Error> {
    if response.status() != reqwest::StatusCode::OK {
        return Err(Error::MediaResponse);
    }
    let headers = response.headers();
    let encodings = headers.get_all("content-encoding");
    let lengths = headers.get_all("content-length");
    let transfers = headers.get_all("transfer-encoding");
    if headers.len() > 32
        || headers
            .iter()
            .map(|(name, value)| name.as_str().len() + value.as_bytes().len())
            .sum::<usize>()
            > 8192
        || encodings.iter().count() > 1
        || encodings
            .iter()
            .next()
            .is_some_and(|value| !value.as_bytes().eq_ignore_ascii_case(b"identity"))
        || lengths.iter().count() > 1
        || transfers.iter().count() > 1
        || transfers.iter().next().is_some_and(|value| {
            !value.as_bytes().eq_ignore_ascii_case(b"chunked") || lengths.iter().next().is_some()
        })
        || headers.contains_key("content-range")
        || response
            .content_length()
            .is_some_and(|declared| declared != length)
    {
        return Err(Error::MediaResponse);
    }
    Ok(())
}
