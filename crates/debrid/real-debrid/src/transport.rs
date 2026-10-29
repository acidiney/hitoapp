use crate::{
    Error,
    network::{self, Dns, DnsPurpose, NetworkError, PinnedResolver, control_client},
};
use reqwest::header::{AUTHORIZATION, HeaderValue};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, OwnedSemaphorePermit, Semaphore},
    time::Instant,
};
use zeroize::Zeroizing;

const MAX_RESPONSE_BYTES: usize = 65_536;
const START_GAP: Duration = Duration::from_millis(250);
const CONNECT_BUDGET: Duration = Duration::from_secs(2);
const API_HOST: &str = "api.real-debrid.com";

pub(crate) struct Transport {
    endpoint: reqwest::Url,
    authorization: HeaderValue,
    dns: Option<Arc<Dns>>,
    slots: Arc<Semaphore>,
    next_start: Mutex<Option<Instant>>,
    #[cfg(any(test, feature = "test-support"))]
    tls: Option<TestTls>,
}

#[cfg(any(test, feature = "test-support"))]
struct TestTls {
    certificate: reqwest::Certificate,
    address: SocketAddr,
}

impl Transport {
    pub(crate) fn new(
        authorization: HeaderValue,
        endpoint: reqwest::Url,
        dns: Arc<Dns>,
    ) -> Result<Self, Error> {
        if endpoint.as_str() != "https://api.real-debrid.com/rest/1.0/" {
            return Err(Error::Configuration);
        }
        Ok(Self::configured(authorization, endpoint, Some(dns)))
    }

    #[cfg(test)]
    pub(crate) fn with_test_endpoint(
        provider_access_token: String,
        endpoint: reqwest::Url,
    ) -> Result<Self, Error> {
        Ok(Self::configured(
            authorization(provider_access_token)?,
            endpoint,
            None,
        ))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn with_test_tls(
        provider_access_token: String,
        endpoint: reqwest::Url,
        ca_der: &[u8],
        address: std::net::SocketAddr,
    ) -> Result<Self, Error> {
        if endpoint.as_str() != "https://api.real-debrid.com/rest/1.0/"
            || !address.ip().is_loopback()
            || address.port() == 0
            || ca_der.len() > 16_384
        {
            return Err(Error::Configuration);
        }
        let certificate =
            reqwest::Certificate::from_der(ca_der).map_err(|_| Error::Configuration)?;
        let mut transport = Self::configured(authorization(provider_access_token)?, endpoint, None);
        transport.tls = Some(TestTls {
            certificate,
            address,
        });
        Ok(transport)
    }

    #[cfg(test)]
    pub(crate) fn with_test_api_dns(
        provider_access_token: String,
        endpoint: reqwest::Url,
        ca_der: &[u8],
        address: SocketAddr,
        dns_address: SocketAddr,
    ) -> Result<Self, Error> {
        let mut transport = Self::with_test_tls(provider_access_token, endpoint, ca_der, address)?;
        transport.dns = Some(Arc::new(
            Dns::with_test_tcp(dns_address).map_err(network_error)?,
        ));
        Ok(transport)
    }

    fn configured(
        authorization: HeaderValue,
        endpoint: reqwest::Url,
        dns: Option<Arc<Dns>>,
    ) -> Self {
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        Self {
            endpoint,
            authorization,
            dns,
            slots: Arc::new(Semaphore::new(2)),
            next_start: Mutex::new(None),
            #[cfg(any(test, feature = "test-support"))]
            tls: None,
        }
    }

    pub(crate) fn admit(&self) -> Result<OwnedSemaphorePermit, Error> {
        self.slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)
    }

    pub(crate) async fn torrent_info(&self, torrent_id: &str) -> Result<Zeroizing<Vec<u8>>, Error> {
        let mut endpoint = self.endpoint.clone();
        endpoint
            .path_segments_mut()
            .map_err(|_| Error::Configuration)?
            .pop_if_empty()
            .extend(["torrents", "info", torrent_id]);
        self.exchange(endpoint, None, None).await
    }

    pub(crate) async fn unrestrict_link(
        &self,
        link: &str,
        post_dispatched: &mut bool,
    ) -> Result<Zeroizing<Vec<u8>>, Error> {
        let mut endpoint = self.endpoint.clone();
        endpoint
            .path_segments_mut()
            .map_err(|_| Error::Configuration)?
            .pop_if_empty()
            .extend(["unrestrict", "link"]);
        self.exchange(endpoint, Some(link), Some(post_dispatched))
            .await
    }

    async fn exchange(
        &self,
        endpoint: reqwest::Url,
        link: Option<&str>,
        post_dispatched: Option<&mut bool>,
    ) -> Result<Zeroizing<Vec<u8>>, Error> {
        let (client, addresses) = self.connection().await?;
        let request = match link {
            Some(link) => client.post(endpoint).form(&[("link", link)]),
            None => client.get(endpoint),
        };
        let request = request
            .header(AUTHORIZATION, self.authorization.clone())
            .header("accept", "application/json")
            .header("accept-encoding", "identity")
            .build()
            .map_err(request_error)?;
        {
            let mut next_start = self.next_start.lock().await;
            if let Some(next_start) = *next_start {
                tokio::time::sleep_until(next_start).await;
            }
            *next_start = Some(Instant::now() + START_GAP);
        }
        if let Some(post_dispatched) = post_dispatched {
            *post_dispatched = true;
        }
        let mut response = client.execute(request).await.map_err(request_error)?;
        if response
            .remote_addr()
            .is_none_or(|peer| !addresses.contains(&peer))
        {
            return Err(Error::Transport);
        }
        if response.status() != reqwest::StatusCode::OK {
            return Err(Error::Status(response.status().as_u16()));
        }
        validate_headers(&response)?;
        let mut bytes = Zeroizing::new(Vec::new());
        while let Some(chunk) = response.chunk().await.map_err(request_error)? {
            if chunk.len() > MAX_RESPONSE_BYTES.saturating_sub(bytes.len()) {
                return Err(Error::ResponseTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    async fn connection(&self) -> Result<(reqwest::Client, Vec<SocketAddr>), Error> {
        let started = Instant::now();
        let addresses = tokio::time::timeout(CONNECT_BUDGET, self.addresses())
            .await
            .map_err(|_| Error::Deadline)??;
        let remaining = CONNECT_BUDGET
            .checked_sub(started.elapsed())
            .filter(|remaining| !remaining.is_zero())
            .ok_or(Error::Deadline)?;
        let builder = control_client(concat!("HitoRealDebrid/", env!("CARGO_PKG_VERSION")))
            .https_only(self.endpoint.scheme() == "https")
            .connect_timeout(remaining)
            .dns_resolver(Arc::new(PinnedResolver::new(
                API_HOST.into(),
                addresses.clone(),
            )));
        #[cfg(any(test, feature = "test-support"))]
        let builder = match &self.tls {
            Some(tls) => builder.tls_certs_only([tls.certificate.clone()]),
            None => builder,
        };
        let client = builder.build().map_err(|_| Error::Configuration)?;
        Ok((client, addresses))
    }

    async fn addresses(&self) -> Result<Vec<SocketAddr>, Error> {
        #[cfg(any(test, feature = "test-support"))]
        if let Some(tls) = &self.tls
            && self.dns.is_none()
        {
            return Ok(vec![tls.address]);
        }
        #[cfg(test)]
        if self.endpoint.scheme() == "http" {
            let address = self
                .endpoint
                .host_str()
                .and_then(|host| host.parse::<std::net::IpAddr>().ok())
                .filter(|address| address.is_loopback())
                .ok_or(Error::Configuration)?;
            let port = self
                .endpoint
                .port_or_known_default()
                .ok_or(Error::Configuration)?;
            return Ok(vec![SocketAddr::new(address, port)]);
        }
        let dns = self.dns.as_ref().ok_or(Error::Configuration)?;
        let resolved = dns
            .addresses(API_HOST, DnsPurpose::Api)
            .await
            .map_err(network_error)?;
        let addresses = network::public_addresses(resolved).map_err(network_error)?;
        #[cfg(any(test, feature = "test-support"))]
        if let Some(tls) = &self.tls {
            return Ok(vec![tls.address]);
        }
        Ok(addresses)
    }
}

pub(crate) fn authorization(provider_access_token: String) -> Result<HeaderValue, Error> {
    let provider_access_token = Zeroizing::new(provider_access_token);
    if provider_access_token.is_empty()
        || provider_access_token.len() > 4096
        || !provider_access_token
            .bytes()
            .all(|byte| (0x21..=0x7e).contains(&byte))
    {
        return Err(Error::InvalidCredential);
    }
    let bearer = Zeroizing::new(format!("Bearer {}", provider_access_token.as_str()));
    let mut authorization = HeaderValue::from_str(&bearer).map_err(|_| Error::InvalidCredential)?;
    authorization.set_sensitive(true);
    Ok(authorization)
}

fn network_error(error: NetworkError) -> Error {
    match error {
        NetworkError::Configuration => Error::Configuration,
        NetworkError::Transport | NetworkError::Destination => Error::Transport,
    }
}

fn request_error(error: reqwest::Error) -> Error {
    if error.is_timeout() {
        Error::Deadline
    } else {
        Error::Transport
    }
}

fn validate_headers(response: &reqwest::Response) -> Result<(), Error> {
    let headers = response.headers();
    let types = headers.get_all("content-type");
    let encodings = headers.get_all("content-encoding");
    let lengths = headers.get_all("content-length");
    let transfers = headers.get_all("transfer-encoding");
    if headers.len() > 32
        || headers
            .iter()
            .map(|(name, value)| name.as_str().len() + value.as_bytes().len())
            .sum::<usize>()
            > 8192
        || types.iter().count() != 1
        || !types
            .iter()
            .next()
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
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
    {
        return Err(Error::InvalidResponse);
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(Error::ResponseTooLarge);
    }
    Ok(())
}
