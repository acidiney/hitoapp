use super::super::{CommunityRankingConfig, source::PublicSource};
use crate::{Error, Result, admission::Accounts};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Semaphore;

pub(in crate::community) struct Transport {
    endpoint: url::Url,
    client: reqwest::Client,
    accounts: Accounts,
    slots: Arc<Semaphore>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ranking {
    ranked: Vec<RankedSource>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RankedSource {
    source: PublicSource,
    successful_reporters: i64,
}

impl Transport {
    pub(in crate::community) fn new(config: CommunityRankingConfig) -> Result<Self> {
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let endpoint = config.endpoint.join("v1/rankings").map_err(|_| invalid())?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .referer(false)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .http1_only()
            .http1_max_headers(32)
            .pool_max_idle_per_host(0)
            .connect_timeout(super::LOOKUP_BUDGET)
            .timeout(super::LOOKUP_BUDGET)
            .user_agent(concat!("Hito/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| {
                Error::Internal("Community ranking transport could not be initialized".into())
            })?;
        Ok(Self {
            endpoint,
            client,
            accounts: Default::default(),
            slots: Arc::new(Semaphore::new(4)),
        })
    }

    pub(super) async fn rank(
        &self,
        account: &str,
        candidates: &[PublicSource],
    ) -> Result<Vec<i64>> {
        let _account = self.accounts.enter(account)?;
        let _global = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| unavailable())?;
        #[derive(Serialize)]
        struct Request<'a> {
            candidates: &'a [PublicSource],
        }
        let mut response = self
            .client
            .post(self.endpoint.clone())
            .header("accept", "application/json")
            .header("accept-encoding", "identity")
            .json(&Request { candidates })
            .send()
            .await
            .map_err(|_| unavailable())?;
        if response.status() != reqwest::StatusCode::OK {
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
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
            if chunk.len() > 4096usize.saturating_sub(bytes.len()) {
                return Err(invalid());
            }
            bytes.extend_from_slice(&chunk);
        }
        let ranking: Ranking = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if ranking.ranked.len() != candidates.len() {
            return Err(invalid());
        }
        let mut scores = vec![None; candidates.len()];
        for ranked in ranking.ranked {
            let index = candidates
                .iter()
                .position(|candidate| candidate == &ranked.source)
                .ok_or_else(invalid)?;
            if ranked.successful_reporters < 0
                || scores[index].replace(ranked.successful_reporters).is_some()
            {
                return Err(invalid());
            }
        }
        scores
            .into_iter()
            .map(|score| score.ok_or_else(invalid))
            .collect()
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
    Error::Upstream("The community service returned an invalid ranking")
}
fn unavailable() -> Error {
    Error::Upstream("Community rankings are unavailable")
}
