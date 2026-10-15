use crate::{Error, Result};
use hito_plugin_sdk::{MANIFEST_PATH, Manifest};
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;

#[derive(Clone)]
pub(super) struct ManifestClient {
    client: reqwest::Client,
    slots: Arc<Semaphore>,
    accounts: super::admission::Accounts,
}

impl ManifestClient {
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
    pub(super) async fn manifest(&self, account: &str, base_url: &str) -> Result<Manifest> {
        let _account = self.accounts.enter(account)?;
        let _permit = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Conflict("Plugin connections are busy. Try again.".into()))?;
        let endpoint = super::offers::endpoint(base_url)?
            .join(MANIFEST_PATH)
            .map_err(|_| Error::Invalid("Invalid plugin service address".into()))?;
        let mut response = self
            .client
            .get(endpoint)
            .header("accept", "application/json")
            .header("accept-encoding", "identity")
            .header("x-hito-plugin-protocol", "1")
            .send()
            .await
            .map_err(|_| Error::Upstream("The plugin service could not be reached"))?;
        let status = response.status();
        tracing::debug!(
            status = status.as_u16(),
            "Plugin manifest response received"
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
                "The plugin service did not return a JSON manifest",
            ));
        }
        if response
            .content_length()
            .is_some_and(|length| length > 65536)
        {
            return Err(Error::Upstream(
                "The plugin manifest exceeds the size limit",
            ));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| Error::Upstream("The plugin manifest could not be read"))?
        {
            if bytes.len() + chunk.len() > 65536 {
                return Err(Error::Upstream(
                    "The plugin manifest exceeds the size limit",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        let manifest: Manifest = serde_json::from_slice(&bytes)
            .map_err(|_| Error::Upstream("The plugin service returned an invalid manifest"))?;
        manifest.validate().map_err(Error::Invalid)?;
        tracing::debug!(
            count = manifest.capabilities.len(),
            "Plugin manifest verified"
        );
        Ok(manifest)
    }
}
