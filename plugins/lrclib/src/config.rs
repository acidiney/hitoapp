use hito_plugin_sdk::server::ServiceAuth;
use url::Url;

pub struct Config {
    pub(crate) endpoint: Url,
    pub(crate) auth: ServiceAuth,
    pub(crate) shutdown: crate::Shutdown,
    pub(crate) contact: Option<String>,
}

#[derive(Debug)]
pub enum InitError {
    ServiceCredential,
    Endpoint,
    HttpClient,
    MetadataContact,
}

impl Config {
    pub fn new(service_bearer: &str) -> Result<Self, InitError> {
        Ok(Self {
            endpoint: Url::parse("https://lrclib.net/").map_err(|_| InitError::Endpoint)?,
            auth: ServiceAuth::new(service_bearer).ok_or(InitError::ServiceCredential)?,
            shutdown: crate::Shutdown::new(),
            contact: None,
        })
    }

    pub fn shutdown_handle(&self) -> crate::Shutdown {
        self.shutdown.clone()
    }

    pub fn with_contact(mut self, contact: &str) -> Result<Self, InitError> {
        if contact.is_empty()
            || contact.len() > 128
            || !contact
                .bytes()
                .all(|byte| byte.is_ascii_graphic() && !matches!(byte, b'(' | b')'))
        {
            return Err(InitError::MetadataContact);
        }
        self.contact = Some(contact.to_owned());
        Ok(self)
    }

    pub fn with_endpoint(mut self, endpoint: Url) -> Result<Self, InitError> {
        let official = endpoint.scheme() == "https"
            && endpoint.host_str() == Some("lrclib.net")
            && endpoint.port().is_none();
        let fixture = endpoint.scheme() == "http"
            && matches!(endpoint.host_str(), Some("127.0.0.1" | "[::1]"));
        if !(official || fixture)
            || endpoint.path() != "/"
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
        {
            return Err(InitError::Endpoint);
        }
        self.endpoint = endpoint;
        Ok(self)
    }
}
