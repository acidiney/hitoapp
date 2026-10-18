use super::{
    config::{CommunityConfig, valid_token},
    source::PublicSource,
};
use crate::{Error, Result};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::time::Duration;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

pub(super) struct Transport {
    pub(super) config: CommunityConfig,
    client: reqwest::Client,
}

pub(super) struct Provisioned {
    pub(super) id: Uuid,
    pub(super) token: Zeroizing<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReporterCredential {
    reporter_id: Uuid,
    token: String,
}
impl Drop for ReporterCredential {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    receipt_id: Uuid,
    submission_id: Uuid,
    status: String,
}

impl Transport {
    pub(super) fn new(config: CommunityConfig) -> Result<Self> {
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .referer(false)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .http1_only()
            .http1_max_headers(32)
            .pool_max_idle_per_host(0)
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_secs(2))
            .user_agent(concat!("Hito/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| Error::Internal("Community transport could not be initialized".into()))?;
        Ok(Self { config, client })
    }

    pub(super) async fn provision(&self, token: &str) -> Result<Provisioned> {
        let mut credential: ReporterCredential = self
            .exchange("v1/admin/reporters", token, &serde_json::json!({}), &[201])
            .await?;
        if credential.reporter_id.get_version_num() != 4 || !valid_token(&credential.token) {
            return Err(invalid());
        }
        Ok(Provisioned {
            id: credential.reporter_id,
            token: Zeroizing::new(std::mem::take(&mut credential.token)),
        })
    }

    pub(super) async fn submit(
        &self,
        token: &str,
        submission_id: Uuid,
        source: &PublicSource,
    ) -> Result<()> {
        #[derive(Serialize)]
        struct Submission<'a> {
            submission_id: Uuid,
            source: &'a PublicSource,
            outcome: &'static str,
        }
        let receipt: Receipt = self
            .exchange(
                "v1/outcomes",
                token,
                &Submission {
                    submission_id,
                    source,
                    outcome: "successful_playback",
                },
                &[200, 201],
            )
            .await?;
        if receipt.submission_id != submission_id
            || receipt.receipt_id.get_version_num() != 4
            || receipt.status != "accepted"
        {
            return Err(invalid());
        }
        Ok(())
    }

    async fn exchange<T: DeserializeOwned>(
        &self,
        path: &'static str,
        token: &str,
        body: &impl Serialize,
        accepted: &[u16],
    ) -> Result<T> {
        if !valid_token(token) {
            return Err(invalid());
        }
        let endpoint = self.config.endpoint.join(path).map_err(|_| invalid())?;
        let mut response = self
            .client
            .post(endpoint)
            .bearer_auth(token)
            .header("accept", "application/json")
            .header("accept-encoding", "identity")
            .json(body)
            .send()
            .await
            .map_err(|_| unavailable())?;
        if !accepted.contains(&response.status().as_u16()) {
            return Err(unavailable());
        }
        let headers = response.headers();
        if headers.len() > 32
            || headers
                .iter()
                .map(|(name, value)| name.as_str().len() + value.len())
                .sum::<usize>()
                > 16_384
            || one(headers, "content-encoding")?.is_some_and(|value| value != "identity")
            || one(headers, "content-type")?
                .and_then(|value| value.split(';').next())
                .map(str::trim)
                != Some("application/json")
            || response
                .content_length()
                .is_some_and(|length| length > 4096)
        {
            return Err(invalid());
        }
        let mut bytes = Zeroizing::new(Vec::new());
        while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
            if chunk.len() > 4096usize.saturating_sub(bytes.len()) {
                return Err(invalid());
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| invalid())
    }
}

fn one<'a>(headers: &'a reqwest::header::HeaderMap, name: &'static str) -> Result<Option<&'a str>> {
    let mut values = headers.get_all(name).iter();
    let value = values
        .next()
        .map(|value| value.to_str().map_err(|_| invalid()))
        .transpose()?;
    if values.next().is_some() {
        return Err(invalid());
    }
    Ok(value)
}

fn invalid() -> Error {
    Error::Upstream("The community service returned an invalid response")
}
fn unavailable() -> Error {
    Error::Upstream("The community service is unavailable")
}
