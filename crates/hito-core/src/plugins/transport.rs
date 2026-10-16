use crate::{Error, Result};
use hito_plugin_sdk::{
    DETAIL_PATH, MANIFEST_PATH, Manifest, ProviderRecording, RecordingRequest, SEARCH_PATH,
    SOURCES_PATH, SearchPage, SearchRequest, SourcePage,
};
use std::{sync::Arc, time::Duration};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

mod source_batch;
pub(crate) use source_batch::SourceBatch;

pub(super) struct Admission {
    _account: OwnedSemaphorePermit,
    _global: OwnedSemaphorePermit,
}

#[derive(Clone)]
pub(super) struct ControlClient {
    client: reqwest::Client,
    slots: Arc<Semaphore>,
    accounts: super::admission::Accounts,
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
            client,
            slots: Arc::new(Semaphore::new(4)),
            accounts: Default::default(),
        })
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
        let body = serde_json::to_value(request)
            .map_err(|_| Error::Internal("Provider search could not be encoded".into()))?;
        let page: SearchPage = self
            .exchange(
                account,
                base_url,
                token,
                Call {
                    path: SEARCH_PATH,
                    body: Some(body),
                    maximum: 262144,
                },
            )
            .await?;
        page.validate(request.limit)
            .map_err(|_| Error::Upstream("The provider returned invalid search results"))?;
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
        recording
            .validate()
            .map_err(|_| Error::Upstream("The provider returned invalid recording details"))?;
        if recording.id != request.recording_id {
            return Err(Error::Upstream(
                "The provider returned a different recording",
            ));
        }
        Ok(recording)
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
            .map_err(|_| Error::Upstream("The provider returned invalid sources"))?;
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
        let account = self.accounts.enter(account)?;
        let global = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Conflict("Plugin connections are busy. Try again.".into()))?;
        Ok(Admission {
            _account: account,
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
            .map_err(|_| Error::Upstream("The plugin service could not be reached"))?;
        let status = response.status();
        tracing::debug!(
            status = status.as_u16(),
            operation = call.path,
            "Plugin response received"
        );
        if !status.is_success() {
            return Err(Error::Upstream(
                "The plugin service rejected the connection",
            ));
        }
        if response
            .headers()
            .get("content-encoding")
            .is_some_and(|value| value != "identity")
        {
            return Err(Error::Upstream(
                "The plugin service returned an unsupported encoding",
            ));
        }
        let json = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .is_some_and(|v| v.trim().eq_ignore_ascii_case("application/json"));
        if !json {
            return Err(Error::Upstream(
                "The plugin service did not return JSON data",
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > maximum as u64)
        {
            return Err(Error::Upstream(
                "The plugin response exceeds the size limit",
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| Error::Upstream("The plugin response could not be read"))?
        {
            if chunk.len() > maximum.saturating_sub(bytes.len()) {
                return Err(Error::Upstream(
                    "The plugin response exceeds the size limit",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        *remaining -= bytes.len();
        serde_json::from_slice(&bytes)
            .map_err(|_| Error::Upstream("The plugin service returned invalid JSON data"))
    }
}

pub(super) struct Call {
    pub(super) path: &'static str,
    pub(super) body: Option<serde_json::Value>,
    pub(super) maximum: usize,
}
