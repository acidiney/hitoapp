use crate::Error;
use reqwest::header::{AUTHORIZATION, HeaderValue};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, OwnedSemaphorePermit, Semaphore},
    time::Instant,
};
use zeroize::Zeroizing;

const MAX_BODY: usize = 65_536;
#[cfg(feature = "test-support")]
const API_HOST: &str = "api.torbox.app";

pub(crate) struct Transport {
    client: reqwest::Client,
    credential: Zeroizing<String>,
    slots: Arc<Semaphore>,
    next_start: Mutex<Option<Instant>>,
}

impl Transport {
    pub(crate) fn new(credential: String) -> Result<Self, Error> {
        Self::configured(Zeroizing::new(credential), builder())
    }

    #[cfg(feature = "test-support")]
    pub(crate) fn with_test_tls(
        credential: String,
        certificate_der: &[u8],
        address: std::net::SocketAddr,
    ) -> Result<Self, Error> {
        let credential = Zeroizing::new(credential);
        if !address.ip().is_loopback()
            || address.port() == 0
            || certificate_der.is_empty()
            || certificate_der.len() > 16_384
        {
            return Err(Error::Configuration);
        }
        let certificate =
            reqwest::Certificate::from_der(certificate_der).map_err(|_| Error::Configuration)?;
        Self::configured(
            credential,
            builder()
                .tls_certs_only([certificate])
                .resolve(API_HOST, address),
        )
    }

    fn configured(
        credential: Zeroizing<String>,
        builder: reqwest::ClientBuilder,
    ) -> Result<Self, Error> {
        if credential.is_empty()
            || credential.len() > 4096
            || !credential.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        {
            return Err(Error::InvalidCredential);
        }
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        Ok(Self {
            client: builder.build().map_err(|_| Error::Configuration)?,
            credential,
            slots: Arc::new(Semaphore::new(2)),
            next_start: Mutex::new(None),
        })
    }

    pub(crate) fn admit(&self) -> Result<OwnedSemaphorePermit, Error> {
        self.slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)
    }

    pub(crate) async fn observe(&self, id: u64) -> Result<Zeroizing<Vec<u8>>, Error> {
        let bearer = Zeroizing::new(format!("Bearer {}", self.credential.as_str()));
        let mut authorization =
            HeaderValue::from_str(&bearer).map_err(|_| Error::InvalidCredential)?;
        authorization.set_sensitive(true);
        let request = self
            .client
            .get(format!(
                "https://api.torbox.app/v1/api/torrents/mylist?id={id}&bypass_cache=true"
            ))
            .header(AUTHORIZATION, authorization)
            .header("accept", "application/json")
            .header("accept-encoding", "identity")
            .build()
            .map_err(|_| Error::Configuration)?;
        self.exchange(request, None).await
    }

    pub(crate) async fn mint_file(
        &self,
        torrent_id: u64,
        file_id: u64,
        dispatched: &mut bool,
    ) -> Result<Zeroizing<Vec<u8>>, Error> {
        let mut target = reqwest::Url::parse("https://api.torbox.app/v1/api/torrents/requestdl")
            .map_err(|_| Error::Configuration)?;
        target
            .query_pairs_mut()
            .append_pair("token", self.credential.as_str())
            .append_pair("torrent_id", &torrent_id.to_string())
            .append_pair("file_id", &file_id.to_string())
            .append_pair("zip_link", "false")
            .append_pair("redirect", "false")
            .append_pair("append_name", "false");
        let request = self
            .client
            .get(target)
            .header("accept", "application/json")
            .header("accept-encoding", "identity")
            .build()
            .map_err(|_| Error::Configuration)?;
        self.exchange(request, Some(dispatched)).await
    }

    async fn exchange(
        &self,
        request: reqwest::Request,
        dispatched: Option<&mut bool>,
    ) -> Result<Zeroizing<Vec<u8>>, Error> {
        {
            let mut start = self.next_start.lock().await;
            if let Some(next) = *start {
                tokio::time::sleep_until(next).await;
            }
            *start = Some(Instant::now() + Duration::from_millis(250));
        }
        if let Some(dispatched) = dispatched {
            *dispatched = true;
        }
        let mut response = self.client.execute(request).await.map_err(request_error)?;
        if response.status() != reqwest::StatusCode::OK {
            return Err(Error::Status(response.status().as_u16()));
        }
        check_headers(&response)?;
        let mut bytes = Zeroizing::new(Vec::new());
        while let Some(chunk) = response.chunk().await.map_err(request_error)? {
            if chunk.len() > MAX_BODY.saturating_sub(bytes.len()) {
                return Err(Error::ResponseTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}

fn builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .pool_max_idle_per_host(0)
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(10))
        .user_agent(concat!("HitoTorBox/", env!("CARGO_PKG_VERSION")))
}

fn request_error(error: reqwest::Error) -> Error {
    if error.is_timeout() {
        Error::Deadline
    } else {
        Error::Transport
    }
}

fn check_headers(response: &reqwest::Response) -> Result<(), Error> {
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
        .is_some_and(|length| length > MAX_BODY as u64)
    {
        return Err(Error::ResponseTooLarge);
    }
    Ok(())
}
