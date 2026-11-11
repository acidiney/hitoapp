use crate::{Config, InitError, error::Error};
use rand::Rng;
use serde::de::DeserializeOwned;
use std::{
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::{sync::Mutex, time::Instant};
use url::Url;
use zeroize::Zeroizing;

const DEADLINE: Duration = Duration::from_secs(6);
const INTERVAL: Duration = Duration::from_secs(1);

#[derive(Default)]
struct Gate {
    next: Option<Instant>,
    blocked: bool,
}

#[derive(Clone)]
pub(crate) struct Client {
    client: reqwest::Client,
    endpoint: Url,
    gate: Arc<Mutex<Gate>>,
}

impl Client {
    pub(crate) fn new(config: Config) -> Result<Self, InitError> {
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let mut user_agent = concat!("HitoLRCLIB/", env!("CARGO_PKG_VERSION")).to_owned();
        if let Some(contact) = &config.contact {
            user_agent.push_str(" (");
            user_agent.push_str(contact);
            user_agent.push(')');
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .https_only(config.endpoint.scheme() == "https")
            .connect_timeout(Duration::from_secs(2))
            .timeout(DEADLINE)
            .user_agent(user_agent)
            .build()
            .map_err(|_| InitError::HttpClient)?;
        Ok(Self {
            client,
            endpoint: config.endpoint,
            gate: Arc::default(),
        })
    }

    pub(crate) async fn get<T: DeserializeOwned>(
        &self,
        path: &str,
        query: &[(&str, &str)],
        maximum: usize,
    ) -> Result<T, Error> {
        let mut endpoint = self
            .endpoint
            .join(path)
            .map_err(|_| Error::InvalidRequest)?;
        if !query.is_empty() {
            endpoint
                .query_pairs_mut()
                .extend_pairs(query.iter().copied());
        }
        tokio::time::timeout(DEADLINE, self.exchange(endpoint, maximum))
            .await
            .map_err(|_| Error::Unavailable)?
    }

    async fn exchange<T: DeserializeOwned>(
        &self,
        endpoint: Url,
        maximum: usize,
    ) -> Result<T, Error> {
        let mut gate = self.gate.try_lock().map_err(|_| Error::Busy)?;
        if gate.blocked {
            return Err(Error::Busy);
        }
        if let Some(next) = gate.next {
            if next.saturating_duration_since(Instant::now()) > Duration::from_secs(4) {
                return Err(Error::Busy);
            }
            tokio::time::sleep_until(next).await;
        }
        gate.next = Some(Instant::now() + INTERVAL);
        let mut response = self
            .client
            .get(endpoint)
            .header("accept", "application/json")
            .header("accept-encoding", "identity")
            .send()
            .await
            .map_err(|_| Error::Unavailable)?;
        if matches!(response.status().as_u16(), 429 | 503) {
            let Some(delay) = crate::cooldown::retry_delay(response.headers(), SystemTime::now())
            else {
                gate.blocked = true;
                tracing::warn!(
                    event = "lrclib_upstream_cooldown_blocked",
                    reason = "invalid_retry_after",
                    recovery = "restart_service"
                );
                return Err(Error::Busy);
            };
            let jitter = rand::thread_rng()
                .gen_range(0..=(delay.as_millis().saturating_mul(3) / 10).min(1000) as u64);
            let delay = delay.saturating_add(Duration::from_millis(jitter));
            match Instant::now().checked_add(delay) {
                Some(next) => gate.next = Some(next),
                None => {
                    gate.blocked = true;
                    tracing::warn!(
                        event = "lrclib_upstream_cooldown_blocked",
                        reason = "retry_after_overflow",
                        recovery = "restart_service"
                    );
                }
            }
            return Err(Error::Busy);
        }
        match response.status().as_u16() {
            200 => {}
            404 => return Err(Error::NotFound),
            _ => return Err(Error::Unavailable),
        }
        let headers = response.headers();
        if headers.len() > 32
            || headers.get_all("content-type").iter().count() != 1
            || !headers
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.split(';').next())
                .is_some_and(|v| v.trim().eq_ignore_ascii_case("application/json"))
            || headers.get_all("content-encoding").iter().count() > 1
            || headers
                .get("content-encoding")
                .is_some_and(|v| !v.as_bytes().eq_ignore_ascii_case(b"identity"))
            || response
                .content_length()
                .is_some_and(|length| length > maximum as u64)
        {
            return Err(Error::InvalidResponse);
        }
        let mut body = Zeroizing::new(Vec::new());
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Unavailable)? {
            if chunk.len() > maximum - body.len() {
                return Err(Error::InvalidResponse);
            }
            body.extend_from_slice(&chunk);
        }
        gate.next = Some(Instant::now() + INTERVAL);
        serde_json::from_slice(&body).map_err(|_| Error::InvalidResponse)
    }
}
