use crate::{Config, InitError, error::Error};
use serde::Deserialize;
use std::time::Duration;
use tokio::sync::Semaphore;
use url::Url;

const MAXIMUM_RESPONSE_BYTES: usize = 65_536;

pub(crate) struct Client {
    http: reqwest::Client,
    api_base: Url,
    slots: Semaphore,
}

#[derive(Deserialize)]
pub(crate) struct Metadata {
    pub id: String,
    pub name: String,
    pub snapshot_id: String,
    pub items: Option<Items>,
}

#[derive(Deserialize)]
pub(crate) struct Items {
    pub total: u32,
}

impl Client {
    pub(crate) fn new(config: Config) -> Result<Self, InitError> {
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(10))
            .user_agent(concat!("HitoSpotifyPlugin/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| InitError::HttpClient)?;
        Ok(Self {
            http,
            api_base: config.api_base,
            slots: Semaphore::new(4),
        })
    }

    pub(crate) async fn playlist(&self, id: &str, token: &str) -> Result<Metadata, Error> {
        let _permit = self.slots.try_acquire().map_err(|_| Error::Busy)?;
        let mut endpoint = self
            .api_base
            .join(&format!("playlists/{id}"))
            .map_err(|_| Error::InvalidResponse)?;
        endpoint
            .query_pairs_mut()
            .append_pair("fields", "id,name,snapshot_id,items(total),external_urls");
        let mut response = self
            .http
            .get(endpoint)
            .bearer_auth(token)
            .header("accept", "application/json")
            .header("accept-encoding", "identity")
            .send()
            .await
            .map_err(|_| Error::Unavailable)?;
        tracing::debug!(
            event = "spotify_response_headers",
            status = response.status().as_u16()
        );
        match response.status() {
            reqwest::StatusCode::OK => {}
            reqwest::StatusCode::UNAUTHORIZED => return Err(Error::Unauthorized),
            reqwest::StatusCode::FORBIDDEN => return Err(Error::AccessDenied),
            reqwest::StatusCode::NOT_FOUND => return Err(Error::NotFound),
            reqwest::StatusCode::TOO_MANY_REQUESTS => return Err(Error::Busy),
            _ => return Err(Error::Unavailable),
        }
        validate_headers(response.headers())?;
        if response
            .content_length()
            .is_some_and(|length| length > MAXIMUM_RESPONSE_BYTES as u64)
        {
            return Err(Error::InvalidResponse);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Unavailable)? {
            if chunk.len() > MAXIMUM_RESPONSE_BYTES - bytes.len() {
                return Err(Error::InvalidResponse);
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| Error::InvalidResponse)
    }
}

fn validate_headers(headers: &reqwest::header::HeaderMap) -> Result<(), Error> {
    let content_types = headers.get_all("content-type");
    if content_types.iter().count() != 1
        || !content_types
            .iter()
            .next()
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
    {
        return Err(Error::InvalidResponse);
    }
    let encodings = headers.get_all("content-encoding");
    if encodings.iter().count() > 1
        || encodings
            .iter()
            .next()
            .is_some_and(|value| !value.as_bytes().eq_ignore_ascii_case(b"identity"))
    {
        return Err(Error::InvalidResponse);
    }
    Ok(())
}
