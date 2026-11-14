use crate::{Error, PlaybackFailureReason, Result, admission::Accounts};
pub(super) mod artwork;
use hito_plugin_sdk::{
    DETAIL_PATH, MANIFEST_PATH, MAX_RECORDING_METADATA_BYTES, METADATA_PATH, Manifest,
    ProviderRecording, RecordingMetadata, RecordingRequest, SEARCH_PATH, SOURCES_PATH, SearchPage,
    SearchRequest, SourcePage,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

mod catalog;
mod catalog_editions;
mod catalog_release_group;
mod catalog_similarity;
mod featured;
pub(super) mod health;
mod lyrics;
mod preparation;
mod refusal;
mod source_batch;
mod source_links;
pub(crate) use source_batch::SourceBatch;

pub(super) struct Admission {
    _account: OwnedSemaphorePermit,
    _global: OwnedSemaphorePermit,
}

#[derive(Clone)]
pub(super) struct ControlClient {
    playback_failures: bool,
    client: reqwest::Client,
    slots: Arc<Semaphore>,
    accounts: Accounts,
    lyrics_slots: Arc<Semaphore>,
    lyrics_accounts: Accounts,
    artwork_slots: Arc<Semaphore>,
    artwork_accounts: Accounts,
    follow_slots: Arc<Semaphore>,
    follow_accounts: Accounts,
    preparation_slots: Arc<Semaphore>,
    preparation_accounts: Accounts,
}

impl ControlClient {
    pub(super) fn new() -> Result<Self> {
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(10))
            .retry(reqwest::retry::never())
            .user_agent(concat!("Hito/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| {
                Error::Internal("Plugin HTTP transport could not be initialized".into())
            })?;
        Ok(Self {
            playback_failures: false,
            client,
            slots: Arc::new(Semaphore::new(4)),
            accounts: Default::default(),
            lyrics_slots: Arc::new(Semaphore::new(4)),
            lyrics_accounts: Default::default(),
            artwork_slots: Arc::new(Semaphore::new(4)),
            artwork_accounts: Default::default(),
            follow_slots: Arc::new(Semaphore::new(4)),
            follow_accounts: Default::default(),
            preparation_slots: Arc::new(Semaphore::new(2)),
            preparation_accounts: Default::default(),
        })
    }

    pub(super) fn for_playback(&self) -> Self {
        let mut client = self.clone();
        client.playback_failures = true;
        client
    }

    fn playback_error(&self, original: Error, reason: PlaybackFailureReason) -> Error {
        if self.playback_failures {
            Error::PlaybackProvider {
                reason,
                original: Box::new(original),
            }
        } else {
            original
        }
    }

    fn invalid_response(&self, message: &'static str) -> Error {
        self.playback_error(
            Error::Upstream(message),
            PlaybackFailureReason::InvalidProviderResponse,
        )
    }

    fn playback_status_error(&self, status: reqwest::StatusCode) -> Error {
        let original = status_error(status);
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return original;
        }
        let reason = match status.as_u16() {
            408 | 429 | 500 | 502 | 503 | 504 => PlaybackFailureReason::ProviderUnavailable,
            400..=499 | 501 | 505 => PlaybackFailureReason::ProviderRefused,
            500..=599 => return original,
            _ => PlaybackFailureReason::InvalidProviderResponse,
        };
        self.playback_error(original, reason)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub(super) async fn manifest(
        &self,
        account: &str,
        base_url: &str,
        token: Option<&str>,
    ) -> Result<Manifest> {
        let manifest: Manifest = self
            .exchange(
                account,
                base_url,
                token,
                Call {
                    path: MANIFEST_PATH,
                    body: None,
                    maximum: 65536,
                },
            )
            .await?;
        manifest.validate().map_err(Error::Invalid)?;
        tracing::debug!(
            count = manifest.capabilities.len(),
            "Plugin manifest verified"
        );
        Ok(manifest)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub(super) async fn search(
        &self,
        account: &str,
        base_url: &str,
        token: Option<&str>,
        request: &SearchRequest,
    ) -> Result<SearchPage> {
        let _admission = self.admit(account)?;
        self.search_admitted(base_url, token, request).await
    }

    pub(super) async fn search_admitted(
        &self,
        base_url: &str,
        token: Option<&str>,
        request: &SearchRequest,
    ) -> Result<SearchPage> {
        let body = serde_json::to_value(request)
            .map_err(|_| Error::Internal("Provider search could not be encoded".into()))?;
        let mut remaining = 262144;
        let page: SearchPage = self
            .exchange_admitted(
                base_url,
                token,
                Call {
                    path: SEARCH_PATH,
                    body: Some(body),
                    maximum: 262144,
                },
                &mut remaining,
            )
            .await?;
        page.validate(request.limit)
            .map_err(|_| self.invalid_response("The provider returned invalid search results"))?;
        Ok(page)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub(super) async fn recording_details(
        &self,
        account: &str,
        base_url: &str,
        token: Option<&str>,
        request: &RecordingRequest,
    ) -> Result<ProviderRecording> {
        let body = serde_json::to_value(request)
            .map_err(|_| Error::Internal("Provider request could not be encoded".into()))?;
        let recording: ProviderRecording = self
            .exchange(
                account,
                base_url,
                token,
                Call {
                    path: DETAIL_PATH,
                    body: Some(body),
                    maximum: 32768,
                },
            )
            .await?;
        recording.validate().map_err(|_| {
            self.invalid_response("The provider returned invalid recording details")
        })?;
        if recording.id != request.recording_id {
            return Err(self.invalid_response("The provider returned a different recording"));
        }
        Ok(recording)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub(super) async fn recording_metadata(
        &self,
        account: &str,
        base_url: &str,
        token: Option<&str>,
        request: &RecordingRequest,
    ) -> Result<RecordingMetadata> {
        let _admission = self.admit(account)?;
        self.recording_metadata_admitted(base_url, token, request)
            .await
    }

    pub(super) async fn recording_metadata_admitted(
        &self,
        base_url: &str,
        token: Option<&str>,
        request: &RecordingRequest,
    ) -> Result<RecordingMetadata> {
        let body = serde_json::to_value(request)
            .map_err(|_| Error::Internal("Provider request could not be encoded".into()))?;
        let mut remaining = MAX_RECORDING_METADATA_BYTES;
        let metadata: RecordingMetadata = self
            .exchange_admitted(
                base_url,
                token,
                Call {
                    path: METADATA_PATH,
                    body: Some(body),
                    maximum: MAX_RECORDING_METADATA_BYTES,
                },
                &mut remaining,
            )
            .await?;
        metadata
            .validate()
            .map_err(|_| self.invalid_response("The provider returned invalid musical metadata"))?;
        if metadata.recording.id != request.recording_id {
            return Err(
                self.invalid_response("The provider returned metadata for a different recording")
            );
        }
        Ok(metadata)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub(super) async fn sources(
        &self,
        account: &str,
        base_url: &str,
        token: Option<&str>,
        request: &RecordingRequest,
    ) -> Result<SourcePage> {
        let _admission = self.admit(account)?;
        self.source_page(base_url, token, request).await
    }

    async fn source_page(
        &self,
        base_url: &str,
        token: Option<&str>,
        request: &RecordingRequest,
    ) -> Result<SourcePage> {
        let body = serde_json::to_value(request)
            .map_err(|_| Error::Internal("Provider request could not be encoded".into()))?;
        let mut remaining = 32768;
        let sources: SourcePage = self
            .exchange_admitted(
                base_url,
                token,
                Call {
                    path: SOURCES_PATH,
                    body: Some(body),
                    maximum: 32768,
                },
                &mut remaining,
            )
            .await?;
        sources
            .validate()
            .map_err(|_| self.invalid_response("The provider returned invalid sources"))?;
        Ok(sources)
    }

    async fn exchange<T: serde::de::DeserializeOwned>(
        &self,
        account: &str,
        base_url: &str,
        token: Option<&str>,
        call: Call,
    ) -> Result<T> {
        let _admission = self.admit(account)?;
        let mut remaining = call.maximum;
        self.exchange_admitted(base_url, token, call, &mut remaining)
            .await
    }

    pub(super) fn admit(&self, account: &str) -> Result<Admission> {
        let account = self
            .accounts
            .try_enter(account)?
            .ok_or(Error::ProviderBusy(
                "Provider requests for this account are busy. Try again.",
            ))?;
        let global = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::ProviderBusy("Plugin connections are busy. Try again."))?;
        Ok(Admission {
            _account: account,
            _global: global,
        })
    }

    pub(super) fn admit_follow(&self, account: &str) -> Result<Admission> {
        let permit = self.follow_accounts.enter(account)?;
        let global = self.follow_slots.clone().try_acquire_owned().map_err(|_| {
            Error::Conflict("Background playlist requests are busy. Try again.".into())
        })?;
        tracing::debug!(account_id = account, "Background playlist request admitted");
        Ok(Admission {
            _account: permit,
            _global: global,
        })
    }

    pub(super) async fn exchange_admitted<T: serde::de::DeserializeOwned>(
        &self,
        base_url: &str,
        token: Option<&str>,
        call: Call,
        remaining: &mut usize,
    ) -> Result<T> {
        self.exchange_optional_admitted(base_url, token, call, remaining, false)
            .await?
            .ok_or(self.invalid_response("The plugin service rejected the request"))
    }

    async fn exchange_optional_admitted<T: serde::de::DeserializeOwned>(
        &self,
        base_url: &str,
        token: Option<&str>,
        call: Call,
        remaining: &mut usize,
        missing_is_unsupported: bool,
    ) -> Result<Option<T>> {
        let maximum = call.maximum.min(*remaining);
        let endpoint = super::offers::endpoint(base_url)?
            .join(call.path)
            .map_err(|_| Error::Invalid("Invalid plugin service address".into()))?;
        let mut request = if call.body.is_some() {
            self.client.post(endpoint)
        } else {
            self.client.get(endpoint)
        };
        if let Some(token) = token {
            super::vault::validate(token)?;
            request = request.bearer_auth(token);
        }
        if let Some(body) = call.body {
            request = request.json(&body);
        }
        let mut response = request
            .header("accept", "application/json")
            .header("accept-encoding", "identity")
            .header("x-hito-plugin-protocol", "1")
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    self.playback_error(
                        Error::Upstream(NOT_RESPONDING),
                        PlaybackFailureReason::ProviderUnavailable,
                    )
                } else {
                    self.playback_error(
                        Error::Upstream(COULD_NOT_REACH),
                        PlaybackFailureReason::ProviderUnavailable,
                    )
                }
            })?;
        let status = response.status();
        tracing::debug!(
            status = status.as_u16(),
            operation = call.path,
            "Plugin response received"
        );
        if missing_is_unsupported && matches!(status.as_u16(), 404 | 501) {
            return Ok(None);
        }
        if !status.is_success() {
            if status.as_u16() == 422 {
                let original = refusal::read(response, maximum).await;
                return Err(self.playback_error(original, PlaybackFailureReason::ProviderRefused));
            }
            return Err(self.playback_status_error(status));
        }
        if response
            .headers()
            .get("content-encoding")
            .is_some_and(|value| value != "identity")
        {
            return Err(
                self.invalid_response("The plugin service returned an unsupported encoding")
            );
        }
        let json = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .is_some_and(|v| v.trim().eq_ignore_ascii_case("application/json"));
        if !json {
            return Err(self.invalid_response("The plugin service did not return JSON data"));
        }
        if response
            .content_length()
            .is_some_and(|length| length > maximum as u64)
        {
            return Err(self.invalid_response("The plugin response exceeds the size limit"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| {
            self.playback_error(
                Error::Upstream("The plugin response could not be read"),
                PlaybackFailureReason::ProviderUnavailable,
            )
        })? {
            if chunk.len() > maximum.saturating_sub(bytes.len()) {
                return Err(self.invalid_response("The plugin response exceeds the size limit"));
            }
            bytes.extend_from_slice(&chunk);
        }
        *remaining -= bytes.len();
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| self.invalid_response("The plugin service returned invalid JSON data"))
    }
}

pub(super) struct Call {
    pub(super) path: &'static str,
    pub(super) body: Option<serde_json::Value>,
    pub(super) maximum: usize,
}

const NOT_RESPONDING: &str =
    "The music service is not responding right now. Try again in a moment.";
const COULD_NOT_REACH: &str = "The plugin service could not be reached";

fn status_error(status: reqwest::StatusCode) -> Error {
    if status == reqwest::StatusCode::UNAUTHORIZED {
        return Error::PluginCredentialsRequired(None);
    }
    Error::Upstream(match status.as_u16() {
        502..=504 => NOT_RESPONDING,
        429 => "The music service is busy. Try again in a moment.",
        403 => {
            "The plugin service refused the request. Check the connection in Settings → Plugins."
        }
        _ => "The plugin service rejected the request",
    })
}
