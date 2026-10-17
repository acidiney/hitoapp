use std::{future::Future, io, net::IpAddr, pin::Pin, sync::Arc};
use url::Url;

mod policy;
mod transport;
pub use policy::PodcastNetworkConfig;
pub(crate) use policy::parse as parse_url;

#[derive(Clone, Default)]
pub(crate) struct ConditionalValidators {
    etag: Option<String>,
    last_modified: Option<String>,
}

pub(crate) struct RemoteResponse {
    pub response: reqwest::Response,
    pub final_url: Url,
    pub permanent_redirect: bool,
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
