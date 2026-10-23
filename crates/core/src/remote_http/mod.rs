use std::{future::Future, io, net::IpAddr, pin::Pin, sync::Arc};
use url::Url;

mod policy;
mod transport;
pub use policy::PodcastNetworkConfig;
pub(crate) use policy::parse as parse_url;

#[derive(Clone, Default)]
pub(crate) struct ConditionalValidators {
    representation_url: Option<Url>,
    etag: Option<String>,
    last_modified: Option<String>,
}

impl ConditionalValidators {
    pub(crate) fn stored(
        representation_url: Option<&str>,
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Self {
        let Some(representation_url) = representation_url.and_then(|url| policy::parse(url).ok())
        else {
            return Self::default();
        };
        Self {
            representation_url: Some(representation_url),
            etag: etag.filter(|value| valid_etag(value)).map(str::to_owned),
            last_modified: last_modified
                .filter(|value| valid_modified(value))
                .map(str::to_owned),
        }
    }

    pub(crate) fn response(url: &Url, headers: &reqwest::header::HeaderMap) -> Self {
        Self::stored(
            Some(url.as_str()),
            single_header(headers, reqwest::header::ETAG),
            single_header(headers, reqwest::header::LAST_MODIFIED),
        )
    }

    pub(crate) fn applies_to(&self, url: &Url) -> bool {
        self.representation_url.as_ref() == Some(url)
            && (self.etag.is_some() || self.last_modified.is_some())
    }

    pub(crate) fn retain_missing(&mut self, previous: &Self) {
        if self.representation_url == previous.representation_url {
            self.etag = self.etag.take().or_else(|| previous.etag.clone());
            self.last_modified = self
                .last_modified
                .take()
                .or_else(|| previous.last_modified.clone());
        }
    }

    pub(crate) fn representation_url(&self) -> Option<&str> {
        self.representation_url.as_ref().map(Url::as_str)
    }

    pub(crate) fn etag(&self) -> Option<&str> {
        self.etag.as_deref()
    }

    pub(crate) fn last_modified(&self) -> Option<&str> {
        self.last_modified.as_deref()
    }
}

fn single_header(
    headers: &reqwest::header::HeaderMap,
    name: reqwest::header::HeaderName,
) -> Option<&str> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?.to_str().ok()?;
    values.next().is_none().then_some(value)
}

fn valid_etag(value: &str) -> bool {
    if value.len() > 1024 || !value.is_ascii() {
        return false;
    }
    let opaque = value.strip_prefix("W/").unwrap_or(value);
    opaque
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .is_some_and(|value| {
            value
                .bytes()
                .all(|byte| byte == 0x21 || (0x23..=0x7e).contains(&byte))
        })
}

fn valid_modified(value: &str) -> bool {
    value.len() == 29
        && chrono::DateTime::parse_from_rfc2822(value)
            .is_ok_and(|date| date.format("%a, %d %b %Y %H:%M:%S GMT").to_string() == value)
}

pub(crate) struct RemoteResponse {
    pub response: reqwest::Response,
    pub final_url: Url,
    pub permanent_redirect: bool,
    pub conditional: bool,
}

type Lookup<'a> = Pin<Box<dyn Future<Output = io::Result<Vec<IpAddr>>> + Send + 'a>>;

trait Resolver: Send + Sync {
    fn resolve<'a>(&'a self, host: &'a str) -> Lookup<'a>;
}

struct SystemResolver;

impl Resolver for SystemResolver {
    fn resolve<'a>(&'a self, host: &'a str) -> Lookup<'a> {
        Box::pin(async move {
            let answers = tokio::net::lookup_host((host, 0)).await?;
            let mut addresses = Vec::new();
            for address in answers {
                if addresses.len() == 32 {
                    return Err(io::Error::other("Remote DNS exceeded its answer bound"));
                }
                addresses.push(address.ip());
            }
            Ok(addresses)
        })
    }
}

#[derive(Clone)]
pub(crate) struct RemoteHttp {
    config: PodcastNetworkConfig,
    resolver: Arc<dyn Resolver>,
}

impl RemoteHttp {
    pub(crate) fn new(config: &PodcastNetworkConfig) -> Self {
        Self {
            config: config.clone(),
            resolver: Arc::new(SystemResolver),
        }
    }

    #[cfg(test)]
    fn with_resolver(config: &PodcastNetworkConfig, resolver: Arc<dyn Resolver>) -> Self {
        Self {
            config: config.clone(),
            resolver,
        }
    }
}

#[cfg(test)]
mod tests;
