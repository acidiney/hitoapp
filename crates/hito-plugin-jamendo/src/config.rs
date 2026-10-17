use url::Url;
use zeroize::Zeroizing;

pub struct Config {
    pub(crate) client_id: Zeroizing<String>,
    pub(crate) endpoint: Url,
    #[cfg(test)]
    pub(crate) fixture_origin: Option<Url>,
}

impl Config {
    pub fn new(client_id: String) -> Result<Self, InitError> {
        let client_id = Zeroizing::new(client_id);
        if client_id.is_empty()
            || client_id.len() > 128
            || !client_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
        {
            return Err(InitError::InvalidClientId);
        }
        Ok(Self {
            client_id,
            endpoint: Url::parse("https://api.jamendo.com/v3.0/tracks/")
                .map_err(|_| InitError::HttpClient)?,
            #[cfg(test)]
            fixture_origin: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn fixture(client_id: String, base: Url) -> Self {
        let mut config = Self::new(client_id).expect("invalid fixture configuration");
        assert_eq!(base.scheme(), "http");
        assert_eq!(base.host_str(), Some("127.0.0.1"));
        config.endpoint = base.join("v3.0/tracks/").unwrap();
        config.fixture_origin = Some(base);
        config
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("The Jamendo client ID is invalid")]
    InvalidClientId,
    #[error("The Jamendo HTTP clients could not be initialized")]
    HttpClient,
}
