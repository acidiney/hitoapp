use crate::Error;
use std::{sync::Arc, time::Duration};
use tokio::{sync::Mutex, time::Instant};
use uuid::Uuid;

const CAA_ROOT: &str = "https://coverartarchive.org/";
const MAX_COVER_BYTES: usize = 2 * 1024 * 1024;
const MAX_REDIRECTS: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoverKind {
    Release,
    ReleaseGroup,
}

impl CoverKind {
    fn path(self) -> &'static str {
        match self {
            Self::Release => "release",
            Self::ReleaseGroup => "release-group",
        }
    }
}

#[derive(Debug)]
pub struct CoverArt {
    pub content_type: &'static str,
    pub bytes: Vec<u8>,
}

/// Reads front covers from the Cover Art Archive by MusicBrainz identifier. Only the identifier
/// leaves the host.
#[derive(Clone)]
pub struct CoverArtClient {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    loopback: bool,
    next_start: Arc<Mutex<Option<Instant>>>,
}

impl CoverArtClient {
    pub fn new() -> Result<Self, Error> {
        Self::build(CAA_ROOT.parse().map_err(|_| Error::Configuration)?, false)
    }

    /// Targets an owned fake service on a literal loopback address. Any other origin is refused.
    #[cfg(any(test, feature = "test-support"))]
    pub fn with_loopback_endpoint(endpoint: reqwest::Url) -> Result<Self, Error> {
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
        {
            return Err(Error::Configuration);
        }
        Self::build(endpoint, true)
    }

    fn build(endpoint: reqwest::Url, loopback: bool) -> Result<Self, Error> {
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(15))
            .retry(reqwest::retry::never())
            .user_agent(concat!(
                "Hito/",
                env!("CARGO_PKG_VERSION"),
                " (contact@example.invalid)"
            ))
            .build()
            .map_err(|_| Error::Configuration)?;
        Ok(Self {
            client,
            endpoint,
            loopback,
            next_start: Arc::new(Mutex::new(None)),
        })
    }

    /// The front cover at 500 pixels, or nothing when the archive holds none.
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "cover_art_front"))]
    pub async fn front(&self, kind: CoverKind, mbid: &str) -> Result<Option<CoverArt>, Error> {
        let canonical = Uuid::parse_str(mbid)
            .ok()
            .filter(|id| id.hyphenated().to_string() == mbid)
            .ok_or(Error::InvalidLookupRequest)?;
        let started = std::time::Instant::now();
        let mut url = self
            .endpoint
            .join(&format!("{}/{canonical}/front-500", kind.path()))
            .map_err(|_| Error::Configuration)?;
        let mut result = Err(Error::Transport);
        for _ in 0..=MAX_REDIRECTS {
            self.wait_for_start().await;
            let response = self
                .client
                .get(url.clone())
                .header("accept", "image/jpeg, image/png")
                .header("accept-encoding", "identity")
                .send()
                .await
                .map_err(|_| Error::Transport)?;
            let status = response.status();
            if status.is_redirection() {
                let next = response
                    .headers()
                    .get("location")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| url.join(value).ok())
                    .filter(|next| self.permitted(next));
                match next {
                    Some(next) => {
                        url = next;
                        continue;
                    }
                    None => {
                        result = Err(Error::InvalidResponse);
                        break;
                    }
                }
            }
            result = self.read(response).await;
            break;
        }
        tracing::debug!(
            outcome = match &result {
                Ok(Some(_)) => "found",
                Ok(None) => "absent",
                Err(error) => error.code(),
            },
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Cover Art Archive lookup completed"
        );
        result
    }

    fn permitted(&self, url: &reqwest::Url) -> bool {
        if self.loopback {
            return url.scheme() == "http"
                && url.host_str() == self.endpoint.host_str()
                && url.port() == self.endpoint.port();
        }
        url.scheme() == "https"
            && url.port().is_none()
            && url.username().is_empty()
            && url
                .host_str()
                .is_some_and(|host| host == "archive.org" || host.ends_with(".archive.org"))
    }

    async fn read(&self, mut response: reqwest::Response) -> Result<Option<CoverArt>, Error> {
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(Error::Status(status.as_u16()));
        }
        if response
            .headers()
            .get("content-encoding")
            .is_some_and(|value| value != "identity")
        {
            return Err(Error::InvalidResponse);
        }
        let content_type = match response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .map(|value| value.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("image/jpeg") => "image/jpeg",
            Some("image/png") => "image/png",
            _ => return Err(Error::InvalidResponse),
        };
        if response
            .content_length()
            .is_some_and(|length| length > MAX_COVER_BYTES as u64)
        {
            return Err(Error::ResponseTooLarge);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Transport)? {
            if chunk.len() > MAX_COVER_BYTES - bytes.len() {
                return Err(Error::ResponseTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.is_empty() {
            return Err(Error::InvalidResponse);
        }
        Ok(Some(CoverArt {
            content_type,
            bytes,
        }))
    }

    async fn wait_for_start(&self) {
        let mut next_start = self.next_start.lock().await;
        if let Some(next_start) = *next_start {
            tokio::time::sleep_until(next_start).await;
        }
        *next_start = Some(Instant::now() + Duration::from_secs(1));
    }
}
