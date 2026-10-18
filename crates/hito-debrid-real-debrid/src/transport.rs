use crate::Error;
use reqwest::header::{AUTHORIZATION, HeaderValue};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{Mutex, OwnedSemaphorePermit, Semaphore},
    time::Instant,
};
use zeroize::Zeroizing;

const MAX_RESPONSE_BYTES: usize = 65_536;
const START_GAP: Duration = Duration::from_millis(250);

pub(crate) struct Transport {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    authorization: HeaderValue,
    slots: Arc<Semaphore>,
    next_start: Mutex<Option<Instant>>,
}

impl Transport {
    pub(crate) fn new(
        provider_access_token: String,
        endpoint: reqwest::Url,
    ) -> Result<Self, Error> {
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
        let mut authorization =
            HeaderValue::from_str(&bearer).map_err(|_| Error::InvalidCredential)?;
        authorization.set_sensitive(true);
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let client = reqwest::Client::builder()
            .https_only(endpoint.scheme() == "https")
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(10))
            .user_agent(concat!("HitoRealDebrid/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| Error::Configuration)?;
        Ok(Self {
            client,
            endpoint,
            authorization,
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

    pub(crate) async fn torrent_info(&self, torrent_id: &str) -> Result<Zeroizing<Vec<u8>>, Error> {
        let mut endpoint = self.endpoint.clone();
        endpoint
            .path_segments_mut()
            .map_err(|_| Error::Configuration)?
            .pop_if_empty()
            .extend(["torrents", "info", torrent_id]);
        self.exchange(self.client.get(endpoint), None).await
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
        self.exchange(
            self.client.post(endpoint).form(&[("link", link)]),
            Some(post_dispatched),
        )
        .await
    }

    async fn exchange(
        &self,
        request: reqwest::RequestBuilder,
        post_dispatched: Option<&mut bool>,
    ) -> Result<Zeroizing<Vec<u8>>, Error> {
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
        let mut response = self.client.execute(request).await.map_err(request_error)?;
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
