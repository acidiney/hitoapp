use crate::{Config, InitError, error::Error};
use axum::http::Method;
use serde::Deserialize;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;
use url::Url;
use zeroize::Zeroizing;

pub(crate) const FORMAT: &str = "mp32";
const MAX_RESPONSE_BYTES: usize = 262_144;

pub(crate) struct Transport {
    control: reqwest::Client,
    media: reqwest::Client,
    endpoint: Url,
    client_id: Zeroizing<String>,
    slots: Arc<Semaphore>,
    #[cfg(test)]
    fixture_origin: Option<Url>,
}

pub(crate) enum Selection<'a> {
    Search { query: &'a str, limit: u16 },
    Track(&'a str),
}

#[derive(Deserialize)]
pub(crate) struct Track {
    pub id: String,
    pub name: String,
    pub artist_name: String,
    pub duration: u64,
    #[serde(default)]
    pub album_id: String,
    #[serde(default)]
    pub album_name: String,
    pub position: Option<u32>,
    #[serde(default)]
    pub audio: String,
}

#[derive(Deserialize)]
struct Page {
    headers: Headers,
    results: Vec<Track>,
}

#[derive(Deserialize)]
struct Headers {
    status: String,
    code: u32,
    results_count: Option<usize>,
}

impl Transport {
    pub(crate) fn new(config: Config) -> Result<Self, InitError> {
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let https_only = config.endpoint.scheme() == "https";
        let builder = || {
            reqwest::Client::builder()
                .https_only(https_only)
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .retry(reqwest::retry::never())
                .connect_timeout(Duration::from_secs(2))
                .user_agent(concat!("HitoJamendoPlugin/", env!("CARGO_PKG_VERSION")))
        };
        let control = builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| InitError::HttpClient)?;
        let media = builder()
            .pool_max_idle_per_host(0)
            .build()
            .map_err(|_| InitError::HttpClient)?;
        Ok(Self {
            control,
            media,
            endpoint: config.endpoint,
            client_id: config.client_id,
            slots: Arc::new(Semaphore::new(16)),
            #[cfg(test)]
            fixture_origin: config.fixture_origin,
        })
    }

    pub(crate) async fn tracks(&self, selection: Selection<'_>) -> Result<Vec<Track>, Error> {
        let _permit = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Busy)?;
        let started = Instant::now();
        let operation = match &selection {
            Selection::Search { .. } => "search",
            Selection::Track(_) => "track",
        };
        let result = self.read(selection).await;
        tracing::debug!(
            event = "jamendo_provider_complete",
            operation,
            outcome = result
                .as_ref()
                .map(|_| "complete")
                .unwrap_or_else(|error| error.outcome()),
            count = result.as_ref().map_or(0, Vec::len),
            elapsed_ms = started.elapsed().as_millis() as u64
        );
        result
    }

    async fn read(&self, selection: Selection<'_>) -> Result<Vec<Track>, Error> {
        let mut endpoint = self.endpoint.clone();
        let maximum = match &selection {
            Selection::Search { limit, .. } => usize::from(*limit),
            Selection::Track(_) => 1,
        };
        {
            let mut query = endpoint.query_pairs_mut();
            query.append_pair("client_id", &self.client_id);
            query.append_pair("format", "json");
            query.append_pair("type", "single albumtrack");
            query.append_pair("audioformat", FORMAT);
            match selection {
                Selection::Search {
                    query: search,
                    limit,
                } => {
                    query.append_pair("search", search);
                    query.append_pair("limit", &limit.to_string());
                    query.append_pair("offset", "0");
                }
                Selection::Track(id) => {
                    query.append_pair("id", id);
                }
            }
        }
        let mut response = self
            .control
            .get(endpoint)
            .header("accept", "application/json")
            .header("accept-encoding", "identity")
            .send()
            .await
            .map_err(request_error)?;
        match response.status() {
            reqwest::StatusCode::OK => {}
            reqwest::StatusCode::NOT_FOUND => return Err(Error::NotFound),
            reqwest::StatusCode::TOO_MANY_REQUESTS => return Err(Error::Busy),
            _ => return Err(Error::Unavailable),
        }
        validate_json_headers(&response)?;
        let mut bytes = Zeroizing::new(Vec::new());
        while let Some(chunk) = response.chunk().await.map_err(request_error)? {
            if chunk.len() > MAX_RESPONSE_BYTES - bytes.len() {
                return Err(Error::InvalidResponse);
            }
            bytes.extend_from_slice(&chunk);
        }
        let page: Page = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidResponse)?;
        match page.headers.code {
            0 if matches!(page.headers.status.as_str(), "success" | "succeed") => {}
            6 => return Err(Error::Busy),
            5 | 11 => return Err(Error::Unavailable),
            _ => return Err(Error::InvalidResponse),
        }
        if page.results.len() > maximum || page.headers.results_count != Some(page.results.len()) {
            return Err(Error::InvalidResponse);
        }
        Ok(page.results)
    }

    pub(crate) fn stream_url(&self, track: &Track) -> Result<Option<Url>, Error> {
        let value = &track.audio;
        if value.is_empty() {
            return Ok(None);
        }
        if value.len() > 8192
            || !value.is_ascii()
            || value.contains(['@', '\\'])
            || value
                .bytes()
                .any(|byte| byte.is_ascii_control() || byte == b' ')
        {
            return Err(Error::InvalidResponse);
        }
        let endpoint = Url::parse(value).map_err(|_| Error::InvalidResponse)?;
        if endpoint.as_str() != value
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(Error::InvalidResponse);
        }
        #[cfg(test)]
        if let Some(origin) = &self.fixture_origin {
            if endpoint.origin() == origin.origin() && endpoint.path() == "/audio/owned.wav" {
                return Ok(Some(endpoint));
            }
            return Err(Error::InvalidResponse);
        }
        if endpoint.scheme() != "https"
            || endpoint.port().is_some()
            || endpoint.host_str() != Some("prod-1.storage.jamendo.com")
            || endpoint.path() != "/"
        {
            return Err(Error::InvalidResponse);
        }
        let ids: Vec<_> = endpoint
            .query_pairs()
            .filter(|(key, _)| key == "trackid")
            .collect();
        let formats: Vec<_> = endpoint
            .query_pairs()
            .filter(|(key, _)| key == "format")
            .collect();
        if ids.len() != 1
            || ids[0].1.as_ref() != track.id.as_str()
            || formats.len() != 1
            || formats[0].1.as_ref() != FORMAT
        {
            return Err(Error::InvalidResponse);
        }
        Ok(Some(endpoint))
    }

    pub(crate) async fn audio(
        &self,
        endpoint: Url,
        method: Method,
        range: Option<String>,
    ) -> Result<reqwest::Response, Error> {
        let mut request = self
            .media
            .request(method, endpoint)
            .header("accept", "audio/mpeg, audio/ogg, audio/flac, audio/wav")
            .header("accept-encoding", "identity");
        if let Some(range) = range {
            request = request.header("range", range);
        }
        tokio::time::timeout(Duration::from_secs(10), request.send())
            .await
            .map_err(|_| Error::Deadline)?
            .map_err(request_error)
    }
}

fn request_error(error: reqwest::Error) -> Error {
    if error.is_timeout() {
        Error::Deadline
    } else {
        Error::Unavailable
    }
}

fn validate_json_headers(response: &reqwest::Response) -> Result<(), Error> {
    let headers = response.headers();
    let types = headers.get_all("content-type");
    let encodings = headers.get_all("content-encoding");
    let lengths = headers.get_all("content-length");
    let transfers = headers.get_all("transfer-encoding");
    if headers.len() > 32
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
        || response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(Error::InvalidResponse);
    }
    Ok(())
}
