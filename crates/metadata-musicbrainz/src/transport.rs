use crate::Error;
use std::{sync::Arc, time::Duration};
use tokio::{sync::Mutex, time::Instant};

const MAX_PROFILE_BODY_BYTES: usize = 65_536;
const MAX_BROWSE_BODY_BYTES: usize = 1_048_576;
const MAX_SEARCH_BODY_BYTES: usize = 262_144;
const MAX_RECORDING_SEARCH_BODY_BYTES: usize = 2_097_152;

#[derive(Clone)]
pub(super) struct Transport {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    next_start: Arc<Mutex<Option<Instant>>>,
}

impl Transport {
    pub(super) fn new(endpoint: reqwest::Url, user_agent: String) -> Result<Self, Error> {
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(10))
            .retry(reqwest::retry::never())
            .user_agent(user_agent)
            .build()
            .map_err(|_| Error::Configuration)?;
        Ok(Self {
            client,
            endpoint,
            next_start: Arc::new(Mutex::new(None)),
        })
    }

    async fn wait_for_start(&self) {
        let waiting = Instant::now();
        let mut next_start = self.next_start.lock().await;
        if let Some(next_start) = *next_start {
            tokio::time::sleep_until(next_start).await;
        }
        *next_start = Some(Instant::now() + Duration::from_secs(1));
        drop(next_start);
        tracing::debug!(
            wait_ms = waiting.elapsed().as_millis() as u64,
            "MusicBrainz request start granted"
        );
    }

    pub(super) async fn artist(&self, id: &str) -> Result<Vec<u8>, Error> {
        let mut endpoint = self
            .endpoint
            .join(&format!("artist/{id}"))
            .map_err(|_| Error::Configuration)?;
        endpoint.query_pairs_mut().append_pair("fmt", "json");
        self.json(endpoint, MAX_PROFILE_BODY_BYTES).await
    }

    pub(super) async fn artist_links(&self, id: &str) -> Result<Vec<u8>, Error> {
        let mut endpoint = self
            .endpoint
            .join(&format!("artist/{id}"))
            .map_err(|_| Error::Configuration)?;
        endpoint
            .query_pairs_mut()
            .extend_pairs([("fmt", "json"), ("inc", "url-rels")]);
        self.json(endpoint, MAX_PROFILE_BODY_BYTES).await
    }

    pub(super) async fn release_groups(
        &self,
        artist_id: &str,
        release_type: Option<&str>,
        limit: u16,
        offset: u32,
    ) -> Result<Vec<u8>, Error> {
        let mut endpoint = self
            .endpoint
            .join("release-group")
            .map_err(|_| Error::Configuration)?;
        endpoint.query_pairs_mut().extend_pairs([
            ("artist", artist_id),
            ("fmt", "json"),
            ("inc", "artist-credits"),
            ("limit", &limit.to_string()),
            ("offset", &offset.to_string()),
        ]);
        if let Some(release_type) = release_type {
            endpoint.query_pairs_mut().append_pair("type", release_type);
        }
        self.json(endpoint, MAX_BROWSE_BODY_BYTES).await
    }

    pub(super) async fn editions(
        &self,
        release_group_id: &str,
        limit: u16,
        offset: u32,
    ) -> Result<Vec<u8>, Error> {
        let mut endpoint = self
            .endpoint
            .join("release")
            .map_err(|_| Error::Configuration)?;
        endpoint.query_pairs_mut().extend_pairs([
            ("release-group", release_group_id),
            ("fmt", "json"),
            ("inc", "artist-credits release-groups"),
            ("limit", &limit.to_string()),
            ("offset", &offset.to_string()),
        ]);
        self.json(endpoint, MAX_BROWSE_BODY_BYTES).await
    }

    pub(super) async fn edition(&self, id: &str) -> Result<Vec<u8>, Error> {
        let mut endpoint = self
            .endpoint
            .join(&format!("release/{id}"))
            .map_err(|_| Error::Configuration)?;
        endpoint.query_pairs_mut().extend_pairs([
            ("fmt", "json"),
            ("inc", "recordings artist-credits release-groups"),
        ]);
        self.json(endpoint, MAX_BROWSE_BODY_BYTES).await
    }

    pub(super) async fn artist_search(
        &self,
        literal: &str,
        limit: u16,
        offset: u32,
    ) -> Result<Vec<u8>, Error> {
        let mut endpoint = self
            .endpoint
            .join("artist")
            .map_err(|_| Error::Configuration)?;
        endpoint.query_pairs_mut().extend_pairs([
            ("query", literal),
            ("fmt", "json"),
            ("limit", &limit.to_string()),
            ("offset", &offset.to_string()),
        ]);
        self.json(endpoint, MAX_SEARCH_BODY_BYTES).await
    }

    pub(super) async fn artist_name_search(&self, literal: &str) -> Result<Vec<u8>, Error> {
        let mut endpoint = self
            .endpoint
            .join("artist")
            .map_err(|_| Error::Configuration)?;
        endpoint.query_pairs_mut().extend_pairs([
            ("query", literal),
            ("fmt", "json"),
            ("limit", &crate::search::NAME_SEARCH_LIMIT.to_string()),
            ("offset", "0"),
        ]);
        self.json(endpoint, MAX_BROWSE_BODY_BYTES).await
    }

    pub(super) async fn recording_search(&self, query: &str) -> Result<Vec<u8>, Error> {
        let mut endpoint = self
            .endpoint
            .join("recording")
            .map_err(|_| Error::Configuration)?;
        endpoint.query_pairs_mut().extend_pairs([
            ("query", query),
            ("fmt", "json"),
            ("limit", &crate::identifiers::PROBE_RESULT_LIMIT.to_string()),
            ("offset", "0"),
        ]);
        self.json(endpoint, MAX_RECORDING_SEARCH_BODY_BYTES).await
    }

    pub(super) async fn isrc(&self, isrc: &str) -> Result<Vec<u8>, Error> {
        let mut endpoint = self
            .endpoint
            .join(&format!("isrc/{isrc}"))
            .map_err(|_| Error::Configuration)?;
        endpoint
            .query_pairs_mut()
            .extend_pairs([("fmt", "json"), ("inc", "artist-credits")]);
        self.json(endpoint, MAX_SEARCH_BODY_BYTES).await
    }

    pub(super) async fn recording_credits(&self, id: &str) -> Result<Vec<u8>, Error> {
        let mut endpoint = self
            .endpoint
            .join(&format!("recording/{id}"))
            .map_err(|_| Error::Configuration)?;
        endpoint
            .query_pairs_mut()
            .extend_pairs([("fmt", "json"), ("inc", "artist-credits")]);
        self.json(endpoint, MAX_SEARCH_BODY_BYTES).await
    }

    pub(super) async fn release_group(&self, id: &str) -> Result<Vec<u8>, Error> {
        let mut endpoint = self
            .endpoint
            .join(&format!("release/{id}"))
            .map_err(|_| Error::Configuration)?;
        endpoint
            .query_pairs_mut()
            .extend_pairs([("fmt", "json"), ("inc", "release-groups")]);
        self.json(endpoint, MAX_PROFILE_BODY_BYTES).await
    }

    pub(super) async fn genres(&self, entity: &str, id: &str) -> Result<Vec<u8>, Error> {
        let mut endpoint = self
            .endpoint
            .join(&format!("{entity}/{id}"))
            .map_err(|_| Error::Configuration)?;
        endpoint
            .query_pairs_mut()
            .extend_pairs([("fmt", "json"), ("inc", "genres")]);
        self.json(endpoint, MAX_SEARCH_BODY_BYTES).await
    }

    pub(super) async fn url_relations(&self, resource: &str) -> Result<Vec<u8>, Error> {
        let mut endpoint = self
            .endpoint
            .join("url")
            .map_err(|_| Error::Configuration)?;
        endpoint.query_pairs_mut().extend_pairs([
            ("resource", resource),
            ("inc", "artist-rels"),
            ("fmt", "json"),
        ]);
        self.json(endpoint, MAX_SEARCH_BODY_BYTES).await
    }

    async fn json(&self, endpoint: reqwest::Url, maximum: usize) -> Result<Vec<u8>, Error> {
        let request = self
            .client
            .get(endpoint)
            .header("accept", "application/json")
            .header("accept-encoding", "identity");
        self.wait_for_start().await;
        let mut response = request.send().await.map_err(|_| Error::Transport)?;
        let status = response.status();
        tracing::debug!(status = status.as_u16(), "MusicBrainz response received");
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
        let json = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"));
        if !json {
            return Err(Error::InvalidResponse);
        }
        if response
            .content_length()
            .is_some_and(|length| length > maximum as u64)
        {
            return Err(Error::ResponseTooLarge);
        }
        let mut bytes = Vec::with_capacity(maximum.min(MAX_PROFILE_BODY_BYTES));
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Transport)? {
            if chunk.len() > maximum.saturating_sub(bytes.len()) {
                return Err(Error::ResponseTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}
