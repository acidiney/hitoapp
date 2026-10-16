use url::Url;

pub struct Config {
    pub(crate) api_base: Url,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_base: Url::parse("https://api.spotify.com/v1/").unwrap(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("The Spotify HTTP client could not be initialized")]
    HttpClient,
}
