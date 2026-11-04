use crate::{Error, Result, admission::Accounts};
use bytes::Bytes;
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use serde::Deserialize;
use std::{
    future::Future,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::Arc,
    time::Duration,
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use url::{Host, Url};
use zeroize::Zeroizing;

const MAX_RESPONSE_BYTES: usize = 16384;

type Lookup = Arc<
    dyn Fn(String) -> Pin<Box<dyn Future<Output = std::io::Result<Vec<IpAddr>>> + Send>>
        + Send
        + Sync,
>;

#[derive(Clone)]
pub(crate) struct TokenClient {
    client: reqwest::Client,
    slots: Arc<Semaphore>,
    accounts: Accounts,
}

pub(super) struct Admission {
    _account: OwnedSemaphorePermit,
    _global: OwnedSemaphorePermit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExchangeFailure {
    Unreachable,
    Blocked,
    Rejected,
    Invalid,
}

impl ExchangeFailure {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Unreachable => "unreachable",
            Self::Blocked => "blocked_address",
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
    access_token: Zeroizing<String>,
    token_type: String,
    #[serde(default)]
    refresh_token: Option<Zeroizing<String>>,
    #[serde(default, deserialize_with = "seconds")]
    expires_in: Option<u64>,
}

fn seconds<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<u64>, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Seconds {
        Number(u64),
        Text(String),
    }
    match Option::<Seconds>::deserialize(deserializer)? {
        None => Ok(None),
        Some(Seconds::Number(value)) => Ok(Some(value)),
        Some(Seconds::Text(value)) => value
            .parse()
            .map(Some)
            .map_err(|_| serde::de::Error::custom("invalid lifetime")),
    }
}

#[derive(Debug)]
struct Blocked;

impl std::fmt::Display for Blocked {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("the token endpoint resolves to a private address")
    }
}

impl std::error::Error for Blocked {}

struct GuardedResolver {
    lookup: Lookup,
}

impl Resolve for GuardedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let lookup = self.lookup.clone();
        Box::pin(async move {
            let found = lookup(name.as_str().to_owned()).await?;
            let allowed: Vec<SocketAddr> = found
                .into_iter()
                .filter(|address| public(*address))
                .map(|address| SocketAddr::new(address, 0))
                .collect();
            if allowed.is_empty() {
                return Err(Box::new(Blocked) as Box<dyn std::error::Error + Send + Sync>);
            }
            Ok(Box::new(allowed.into_iter()) as Addrs)
        })
    }
}

fn system_lookup() -> Lookup {
    Arc::new(|host| {
        Box::pin(async move {
            let found = tokio::net::lookup_host((host.as_str(), 0)).await?;
            Ok(found.map(|address| address.ip()).collect())
        })
    })
}

fn public(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            let octets = address.octets();
            !(address.is_unspecified()
                || address.is_loopback()
                || address.is_private()
                || address.is_link_local()
                || address.is_broadcast()
                || address.is_multicast()
                || address.is_documentation()
                || octets[0] == 0
                || (octets[0] == 100 && octets[1] & 0xc0 == 64)
                || (octets[0] == 192 && octets[1] == 0 && octets[2] == 0)
                || (octets[0] == 198 && octets[1] & 0xfe == 18)
                || octets[0] >= 240)
        }
        IpAddr::V6(address) => {
            if let Some(embedded) = address.to_ipv4() {
                return public(IpAddr::V4(embedded));
            }
            let segments = address.segments();
            !(address.is_unspecified()
                || address.is_loopback()
                || address.is_multicast()
                || segments[0] & 0xfe00 == 0xfc00
                || segments[0] & 0xffc0 == 0xfe80
                || (segments[0] == 0x2001 && segments[1] == 0x0db8)
                || (segments[0] == 0x0064 && segments[1] == 0xff9b))
        }
    }
}

fn literal_allowed(endpoint: &str) -> bool {
    match Url::parse(endpoint)
        .ok()
        .and_then(|url| url.host().map(|host| host.to_owned()))
    {
        Some(Host::Ipv4(address)) => address.is_loopback() || public(IpAddr::V4(address)),
        Some(Host::Ipv6(address)) => address.is_loopback() || public(IpAddr::V6(address)),
        Some(Host::Domain(_)) => true,
        None => false,
    }
}

fn caused_by_block(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(error) = current {
        if error.is::<Blocked>() {
            return true;
        }
        current = error.source();
    }
    false
}

impl TokenClient {
    pub(crate) fn new() -> Result<Self> {
        Self::with_lookup(system_lookup())
    }

    fn with_lookup(lookup: Lookup) -> Result<Self> {
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .dns_resolver(Arc::new(GuardedResolver { lookup }))
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
            accounts: Accounts::new(1),
        })
    }

    pub(super) fn admit(&self, account: &str) -> Result<Admission> {
        let busy = || Error::ProviderBusy("Provider connections are busy. Try again in a moment.");
        let account = self.accounts.try_enter(account)?.ok_or_else(busy)?;
        let global = self.slots.clone().try_acquire_owned().map_err(|_| busy())?;
        Ok(Admission {
            _account: account,
            _global: global,
        })
    }

    pub(super) async fn redeem(
        &self,
        redemption: Redemption<'_>,
    ) -> std::result::Result<GrantSummary, ExchangeFailure> {
        if !literal_allowed(redemption.endpoint) {
            return Err(ExchangeFailure::Blocked);
        }
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
            .body(Bytes::from_owner(body))
            .send()
            .await
            .map_err(|error| {
                if caused_by_block(&error) {
                    ExchangeFailure::Blocked
                } else {
                    ExchangeFailure::Unreachable
                }
            })?;
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
            .is_none_or(|value| value.as_bytes().eq_ignore_ascii_case(b"identity"));
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
        let graphic = |value: &str| {
            !value.is_empty() && value.len() <= 4096 && value.bytes().all(|b| b.is_ascii_graphic())
        };
        if !graphic(&grant.access_token)
            || !grant.token_type.eq_ignore_ascii_case("bearer")
            || grant
                .refresh_token
                .as_ref()
                .is_some_and(|token| !graphic(token))
        {
            return Err(ExchangeFailure::Invalid);
        }
        Ok(GrantSummary {
            expires_in: grant.expires_in,
            has_refresh_token: grant.refresh_token.is_some(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_unicast_addresses_are_permitted() {
        for blocked in [
            "0.0.0.0",
            "0.1.2.3",
            "10.0.0.1",
            "100.64.0.1",
            "100.127.255.255",
            "127.0.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "172.31.255.255",
            "192.0.0.1",
            "192.0.2.1",
            "192.168.0.1",
            "198.18.0.1",
            "198.19.255.255",
            "224.0.0.1",
            "240.0.0.1",
            "255.255.255.255",
            "::",
            "::1",
            "::ffff:10.0.0.1",
            "::ffff:127.0.0.1",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "febf::1",
            "ff02::1",
            "2001:db8::1",
            "64:ff9b::1",
        ] {
            assert!(!public(blocked.parse().unwrap()), "{blocked}");
        }
        for allowed in [
            "8.8.8.8",
            "1.1.1.1",
            "100.63.255.255",
            "100.128.0.1",
            "172.15.0.1",
            "172.32.0.1",
            "198.17.0.1",
            "198.20.0.1",
            "2606:4700:4700::1111",
            "2001:4860:4860::8888",
            "::ffff:8.8.8.8",
        ] {
            assert!(public(allowed.parse().unwrap()), "{allowed}");
        }
    }

    #[tokio::test]
    async fn a_hostname_resolving_to_a_private_address_is_refused_before_any_connection() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        for answer in [
            "127.0.0.1",
            "10.1.2.3",
            "169.254.169.254",
            "fd00::5",
            "::ffff:192.168.1.1",
        ] {
            let answer: IpAddr = answer.parse().unwrap();
            let lookup: Lookup = Arc::new(move |_| Box::pin(async move { Ok(vec![answer]) }));
            let client = TokenClient::with_lookup(lookup).unwrap();
            let endpoint = format!("https://rebinding.example.test:{port}/token");
            let result = client
                .redeem(Redemption {
                    endpoint: &endpoint,
                    client_id: "owned-client",
                    redirect_uri: "https://hito.test/callback",
                    code: "owned-code",
                    verifier: "owned-verifier",
                })
                .await;
            assert_eq!(result.err(), Some(ExchangeFailure::Blocked), "{answer}");
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err(),
            "the resolver must refuse before any connection is made"
        );
    }

    #[tokio::test]
    async fn a_lookup_failure_is_unreachable_not_blocked() {
        let lookup: Lookup =
            Arc::new(|_| Box::pin(async { Err(std::io::Error::other("owned lookup failure")) }));
        let client = TokenClient::with_lookup(lookup).unwrap();
        let result = client
            .redeem(Redemption {
                endpoint: "https://missing.example.test/token",
                client_id: "c",
                redirect_uri: "https://hito.test/callback",
                code: "c",
                verifier: "v",
            })
            .await;
        assert_eq!(result.err(), Some(ExchangeFailure::Unreachable));
    }
}
