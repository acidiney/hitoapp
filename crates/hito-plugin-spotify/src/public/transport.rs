use crate::{Config, InitError, error::Error};
use serde_json::json;
use std::time::Duration;
use url::Url;
use zeroize::Zeroizing;

const QUERY_HASH: &str = "c685ca91fb0019c87ff1225f84e1d85b26cd631fa6187f3f0900da182cb0953d";
const MAX_BOOTSTRAP_BYTES: usize = 2 * 1024 * 1024;
const MAX_PAGE_BYTES: usize = 1024 * 1024;

pub(super) struct Transport {
    http: reqwest::Client,
    public_base: Url,
    query_endpoint: Url,
}

impl Transport {
    pub(super) fn new(config: &Config) -> Result<Self, InitError> {
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
            public_base: config.public_base.clone(),
            query_endpoint: config.query_endpoint.clone(),
        })
    }

    pub(super) async fn bootstrap(&self, id: &str) -> Result<Zeroizing<Vec<u8>>, Error> {
        let endpoint = self
            .public_base
            .join(&format!("embed/playlist/{id}"))
            .map_err(|_| Error::InvalidResponse)?;
        let response = self
            .http
            .get(endpoint)
            .header("accept", "text/html")
            .header("accept-encoding", "identity")
            .send()
            .await
            .map_err(|_| Error::Unavailable)?;
        read_response(response, "text/html", MAX_BOOTSTRAP_BYTES).await
    }

    pub(super) async fn page(
        &self,
        id: &str,
        token: &str,
        offset: usize,
    ) -> Result<Zeroizing<Vec<u8>>, Error> {
        let response = self
            .http
            .post(self.query_endpoint.clone())
            .bearer_auth(token)
            .header("accept", "application/json")
            .header("accept-encoding", "identity")
            .json(&json!({
                "operationName": "queryPlaylist",
                "variables": {
                    "uri": format!("spotify:playlist:{id}"),
                    "limit": 100,
                    "offset": offset,
                    "includeItemV3": false
                },
                "extensions": {"persistedQuery": {
                    "version": 1,
                    "sha256Hash": QUERY_HASH
                }}
            }))
            .send()
            .await
            .map_err(|_| Error::Unavailable)?;
        read_response(response, "application/json", MAX_PAGE_BYTES).await
    }
}

async fn read_response(
    mut response: reqwest::Response,
    content_type: &str,
    maximum: usize,
) -> Result<Zeroizing<Vec<u8>>, Error> {
    tracing::debug!(
        event = "spotify_response_headers",
        status = response.status().as_u16()
    );
    match response.status() {
        reqwest::StatusCode::OK => {}
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN => {
            return Err(Error::PublicAccessDenied);
        }
        reqwest::StatusCode::NOT_FOUND => return Err(Error::NotFound),
        reqwest::StatusCode::TOO_MANY_REQUESTS => return Err(Error::Busy),
        _ => return Err(Error::Unavailable),
    }
    let headers = response.headers();
    let types = headers.get_all("content-type");
    let encodings = headers.get_all("content-encoding");
    if types.iter().count() != 1
        || !types
            .iter()
            .next()
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .is_some_and(|value| value.trim().eq_ignore_ascii_case(content_type))
        || encodings.iter().count() > 1
        || encodings
            .iter()
            .next()
            .is_some_and(|value| !value.as_bytes().eq_ignore_ascii_case(b"identity"))
        || response
            .content_length()
            .is_some_and(|length| length > maximum as u64)
    {
        return Err(Error::InvalidResponse);
    }
    let mut bytes = Zeroizing::new(Vec::new());
    while let Some(chunk) = response.chunk().await.map_err(|_| Error::Unavailable)? {
        if chunk.len() > maximum - bytes.len() {
            return Err(Error::InvalidResponse);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
