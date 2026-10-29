use std::time::Duration;
use url::Url;

const LABS: &str = "https://labs.api.listenbrainz.org/";

pub struct Config {
    pub(crate) endpoint: Url,
    pub(crate) min_interval: Duration,
}

#[derive(Debug)]
pub enum InitError {
    InvalidEndpoint,
    HttpClient,
}

impl Config {
    /// The public ListenBrainz labs service, at most one request per second.
    pub fn production() -> Result<Self, InitError> {
        Ok(Self {
            endpoint: Url::parse(LABS).map_err(|_| InitError::InvalidEndpoint)?,
            min_interval: Duration::from_secs(1),
        })
    }

    /// A different labs-compatible origin. Only HTTPS, or HTTP on loopback for local fakes.
    pub fn with_endpoint(endpoint: Url) -> Result<Self, InitError> {
        let loopback = matches!(
            endpoint.host_str(),
            Some("127.0.0.1" | "localhost" | "[::1]")
        );
        if !(endpoint.scheme() == "https" || (endpoint.scheme() == "http" && loopback))
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || !endpoint.username().is_empty()
        {
            return Err(InitError::InvalidEndpoint);
        }
        let min_interval = if loopback {
            Duration::ZERO
        } else {
            Duration::from_secs(1)
        };
        Ok(Self {
            endpoint,
            min_interval,
        })
    }
}
