use super::{ConditionalValidators, RemoteHttp, RemoteResponse, policy};
use crate::{AudioRequest, Error, Result};
use reqwest::{Client, Method, StatusCode, header};
use std::{
    net::{IpAddr, SocketAddr},
    time::{Duration, Instant},
};
use url::{Host, Url};

enum Purpose {
    Feed(ConditionalValidators),
    Audio(AudioRequest),
}

impl RemoteHttp {
    #[tracing::instrument(level = "debug", skip_all, fields(request_id = %uuid::Uuid::new_v4()))]
    pub(crate) async fn get(
        &self,
        url: &str,
        validators: ConditionalValidators,
    ) -> Result<RemoteResponse> {
        self.request(url, Purpose::Feed(validators)).await
    }

    #[tracing::instrument(level = "debug", skip_all, fields(request_id = %uuid::Uuid::new_v4()))]
    pub(crate) async fn audio(&self, url: &str, request: AudioRequest) -> Result<RemoteResponse> {
        self.request(url, Purpose::Audio(request)).await
    }

    async fn request(&self, url: &str, purpose: Purpose) -> Result<RemoteResponse> {
        let started = Instant::now();
        let result = tokio::time::timeout(Duration::from_secs(10), self.exchange(url, purpose))
            .await
            .unwrap_or(Err(Error::Upstream("The remote podcast request timed out")));
        tracing::debug!(
            event = "remote_http_finished",
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |_| "complete"),
            status = result
                .as_ref()
                .map_or(0, |response| response.response.status().as_u16()),
            permanent_redirect = result
                .as_ref()
                .is_ok_and(|response| response.permanent_redirect),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Remote podcast request finished"
        );
        result
    }

    async fn exchange(&self, value: &str, purpose: Purpose) -> Result<RemoteResponse> {
        let mut target = policy::parse(value)?;
        let mut permanent = true;
        for followed in 0..=5 {
            let addresses = self.addresses(&target).await?;
            if !self.config.permits(&target, &addresses) {
                return Err(Error::Invalid(
                    "The remote podcast destination is not permitted".into(),
                ));
            }
            let client = pinned_client(&target, &addresses)?;
            let (method, accept) = match &purpose {
                Purpose::Feed(_) => (
                    Method::GET,
                    "application/rss+xml, application/atom+xml, application/xml, text/xml;q=0.9",
                ),
                Purpose::Audio(request) => (
                    if request.head {
                        Method::HEAD
                    } else {
                        Method::GET
                    },
                    "audio/*,application/ogg",
                ),
            };
            let mut outgoing = client
                .request(method, target.clone())
                .header(header::ACCEPT, accept)
                .header(header::ACCEPT_ENCODING, "identity")
                .header(header::CONNECTION, "close");
            let conditional =
                matches!(&purpose, Purpose::Feed(validators) if validators.applies_to(&target));
            match &purpose {
                Purpose::Feed(validators) if conditional => {
                    if let Some(etag) = &validators.etag {
                        outgoing = outgoing.header(header::IF_NONE_MATCH, etag);
                    }
                    if let Some(modified) = &validators.last_modified {
                        outgoing = outgoing.header(header::IF_MODIFIED_SINCE, modified);
                    }
                }
                Purpose::Audio(request) if !request.head => {
                    if let Some(range) = request.range {
                        outgoing = outgoing.header(header::RANGE, range.header());
                    }
                }
                _ => {}
            }
            let response = outgoing
                .send()
                .await
                .map_err(|_| Error::Upstream("The remote podcast request failed"))?;
            let peer = response.remote_addr().ok_or(Error::Upstream(
                "The remote podcast connection is unavailable",
            ))?;
            if !addresses.contains(&peer.ip())
                || Some(peer.port()) != target.port_or_known_default()
            {
                return Err(Error::Upstream("The remote podcast connection changed"));
            }
            if !matches!(
                response.status(),
                StatusCode::MOVED_PERMANENTLY
                    | StatusCode::FOUND
                    | StatusCode::SEE_OTHER
                    | StatusCode::TEMPORARY_REDIRECT
                    | StatusCode::PERMANENT_REDIRECT
            ) {
                return Ok(RemoteResponse {
                    response,
                    final_url: target,
                    permanent_redirect: followed > 0 && permanent,
                    conditional,
                });
            }
            if followed == 5 {
                return Err(Error::Upstream(
                    "The remote podcast redirected too many times",
                ));
            }
            permanent &= matches!(
                response.status(),
                StatusCode::MOVED_PERMANENTLY | StatusCode::PERMANENT_REDIRECT
            );
            let mut locations = response.headers().get_all(header::LOCATION).iter();
            let location = locations
                .next()
                .and_then(|value| value.to_str().ok())
                .ok_or(Error::Upstream("The remote podcast redirect is invalid"))?;
            if locations.next().is_some() {
                return Err(Error::Upstream("The remote podcast redirect is invalid"));
            }
            target = policy::redirect(&target, location)?;
        }
        Err(Error::Upstream(
            "The remote podcast redirected too many times",
        ))
    }

    async fn addresses(&self, target: &Url) -> Result<Vec<IpAddr>> {
        match target.host().ok_or(Error::Invalid(
            "The remote podcast address is invalid".into(),
        ))? {
            Host::Ipv4(address) => Ok(vec![IpAddr::V4(address)]),
            Host::Ipv6(address) => Ok(vec![IpAddr::V6(address)]),
            Host::Domain(host) => {
                tokio::time::timeout(Duration::from_secs(2), self.resolver.resolve(host))
                    .await
                    .map_err(|_| Error::Upstream("The remote podcast DNS request timed out"))?
                    .map_err(|_| Error::Upstream("The remote podcast DNS request failed"))
            }
        }
    }
}

fn pinned_client(target: &Url, addresses: &[IpAddr]) -> Result<Client> {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    let host = target
        .host_str()
        .ok_or(Error::Upstream("The remote podcast host is unavailable"))?;
    let port = target
        .port_or_known_default()
        .ok_or(Error::Upstream("The remote podcast port is invalid"))?;
    let sockets = addresses
        .iter()
        .map(|address| SocketAddr::new(*address, port))
        .collect::<Vec<_>>();
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .referer(false)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .http1_only()
        .http1_max_headers(32)
        .pool_max_idle_per_host(0)
        .connect_timeout(Duration::from_secs(2))
        .user_agent(concat!("Hito/", env!("CARGO_PKG_VERSION")))
        .resolve_to_addrs(host, &sockets)
        .build()
        .map_err(|_| Error::Internal("Remote podcast transport could not be initialized".into()))
}
