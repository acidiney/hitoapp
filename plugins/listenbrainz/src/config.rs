use hito_plugin_sdk::server::ServiceAuth;
use std::time::Duration;
use url::Url;

const LABS: &str = "https://labs.api.listenbrainz.org/";

pub struct Config {
    pub(crate) endpoint: Url,
    pub(crate) min_interval: Duration,
    pub(crate) contact: String,
    pub(crate) auth: ServiceAuth,
}

#[derive(Debug)]
pub enum InitError {
    InvalidEndpoint,
    InvalidContact,
    InvalidServiceToken,
    HttpClient,
}

impl Config {
    /// The public ListenBrainz labs service, at most one request per second.
    ///
    /// `contact` is the operator's email address or web address, sent in the User-Agent as the
    /// upstream rules require. `service_bearer` is the secret Hito presents on every request.
    pub fn new(contact: &str, service_bearer: &str) -> Result<Self, InitError> {
        let email = contact.contains('@') && !contact.contains("://");
        let web = contact.starts_with("https://");
        if !(email || web)
            || contact.len() > 128
            || !contact
                .bytes()
                .all(|byte| (0x21..=0x7e).contains(&byte) && byte != b'(' && byte != b')')
        {
            return Err(InitError::InvalidContact);
        }
        let auth = ServiceAuth::new(service_bearer).ok_or(InitError::InvalidServiceToken)?;
        Ok(Self {
            endpoint: Url::parse(LABS).map_err(|_| InitError::InvalidEndpoint)?,
            min_interval: Duration::from_secs(1),
            contact: contact.to_owned(),
            auth,
        })
    }

    /// A different labs-compatible origin. Only HTTPS, or HTTP on loopback for local fakes.
    pub fn with_endpoint(mut self, endpoint: Url) -> Result<Self, InitError> {
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
        self.endpoint = endpoint;
        Ok(self)
    }

    pub fn with_min_interval(mut self, interval: Duration) -> Self {
        self.min_interval = interval;
        self
    }
}
