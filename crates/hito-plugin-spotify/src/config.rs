use url::Url;

pub struct Config {
    pub(crate) api_base: Url,
    pub(crate) public_base: Url,
    pub(crate) query_endpoint: Url,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_base: Url::parse("https://api.spotify.com/v1/").unwrap(),
            public_base: Url::parse("https://open.spotify.com/").unwrap(),
            query_endpoint: Url::parse("https://api-partner.spotify.com/pathfinder/v2/query")
                .unwrap(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("The Spotify HTTP client could not be initialized")]
    HttpClient,
}
