use crate::{Error, Result};
use serde::Deserialize;
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;
use zeroize::Zeroizing;

const MAX_RESPONSE_BYTES: usize = 16384;

#[derive(Clone)]
pub(crate) struct TokenClient {
    client: reqwest::Client,
    slots: Arc<Semaphore>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExchangeFailure {
    Unreachable,
    Rejected,
    Invalid,
}

impl ExchangeFailure {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Unreachable => "unreachable",
            Self::Rejected => "rejected",
            Self::Invalid => "invalid_response",
        }
    }
}

pub(super) struct GrantSummary {
    pub(super) expires_in: Option<u64>,
    pub(super) has_refresh_token: bool,
}

pub(super) struct Redemption<'a> {
    pub(super) endpoint: &'a str,
    pub(super) client_id: &'a str,
    pub(super) redirect_uri: &'a str,
    pub(super) code: &'a str,
    pub(super) verifier: &'a str,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

impl TokenClient {
    pub(crate) fn new() -> Result<Self> {
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
                Error::Internal("Provider authorization transport could not be initialized".into())
            })?;
        Ok(Self {
            client,
            slots: Arc::new(Semaphore::new(4)),
        })
    }

    pub(super) fn admit(&self) -> Result<tokio::sync::OwnedSemaphorePermit> {
        self.slots.clone().try_acquire_owned().map_err(|_| {
            Error::ProviderBusy("Provider connections are busy. Try again in a moment.")
        })
    }

    pub(super) async fn redeem(
        &self,
        redemption: Redemption<'_>,
    ) -> std::result::Result<GrantSummary, ExchangeFailure> {
        let body = Zeroizing::new(
            url::form_urlencoded::Serializer::new(String::new())
                .append_pair("grant_type", "authorization_code")
                .append_pair("code", redemption.code)
                .append_pair("redirect_uri", redemption.redirect_uri)
                .append_pair("client_id", redemption.client_id)
                .append_pair("code_verifier", redemption.verifier)
                .finish(),
        );
        let mut response = self
            .client
            .post(redemption.endpoint)
            .header("accept", "application/json")
            .header("accept-encoding", "identity")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body.to_string())
            .send()
            .await
            .map_err(|_| ExchangeFailure::Unreachable)?;
        let status = response.status();
        tracing::debug!(status = status.as_u16(), "Provider token response received");
        if !status.is_success() {
            return Err(ExchangeFailure::Rejected);
        }
        let json = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"));
        let plain = response
            .headers()
            .get("content-encoding")
            .is_none_or(|value| value == "identity");
        if !json
            || !plain
            || response
                .content_length()
                .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(ExchangeFailure::Invalid);
        }
        let mut body = Zeroizing::new(Vec::new());
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| ExchangeFailure::Unreachable)?
        {
            if chunk.len() > MAX_RESPONSE_BYTES - body.len() {
                return Err(ExchangeFailure::Invalid);
            }
            body.extend_from_slice(&chunk);
        }
        let grant: TokenResponse =
            serde_json::from_slice(&body).map_err(|_| ExchangeFailure::Invalid)?;
        let access_token = Zeroizing::new(grant.access_token);
        let refresh_token = grant.refresh_token.map(Zeroizing::new);
        let graphic = |value: &str| {
            !value.is_empty() && value.len() <= 4096 && value.bytes().all(|b| b.is_ascii_graphic())
        };
        if !graphic(&access_token)
            || !grant.token_type.eq_ignore_ascii_case("bearer")
            || refresh_token.as_ref().is_some_and(|token| !graphic(token))
        {
            return Err(ExchangeFailure::Invalid);
        }
        Ok(GrantSummary {
            expires_in: grant.expires_in,
            has_refresh_token: refresh_token.is_some(),
        })
    }
}
