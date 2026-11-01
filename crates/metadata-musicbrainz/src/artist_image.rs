use crate::{CoverArt, Error, MusicBrainzClient, cover_art::read_image, identifiers};
use serde_json::Value;
use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::{sync::Mutex, time::Instant};

const WIKIDATA_ROOT: &str = "https://www.wikidata.org/";
const COMMONS_ROOT: &str = "https://commons.wikimedia.org/";
const MAX_ENTITY_BYTES: usize = 4 * 1024 * 1024;
const MAX_REDIRECTS: usize = 3;
const MAX_FILE_NAME_BYTES: usize = 240;
const THUMBNAIL_WIDTH: &str = "320";
const REQUEST_SPACING: Duration = Duration::from_millis(500);
const ALLOWED_HOSTS: [&str; 4] = [
    "www.wikidata.org",
    "wikidata.org",
    "commons.wikimedia.org",
    "upload.wikimedia.org",
];

#[derive(Clone)]
pub struct ArtistImageClient {
    musicbrainz: MusicBrainzClient,
    client: Arc<OnceLock<reqwest::Client>>,
    wikidata: reqwest::Url,
    commons: reqwest::Url,
    loopback: bool,
    next_start: Arc<Mutex<Option<Instant>>>,
}

impl ArtistImageClient {
    pub fn new(musicbrainz: MusicBrainzClient) -> Result<Self, Error> {
        Ok(Self::build(
            musicbrainz,
            WIKIDATA_ROOT.parse().map_err(|_| Error::Configuration)?,
            COMMONS_ROOT.parse().map_err(|_| Error::Configuration)?,
            false,
        ))
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn with_loopback_endpoint(
        musicbrainz: MusicBrainzClient,
        endpoint: reqwest::Url,
    ) -> Result<Self, Error> {
        let loopback = endpoint
            .host_str()
            .and_then(|host| host.parse::<std::net::IpAddr>().ok())
            .is_some_and(|address| address.is_loopback());
        if endpoint.scheme() != "http"
            || !loopback
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(Error::Configuration);
        }
        Ok(Self::build(musicbrainz, endpoint.clone(), endpoint, true))
    }

    fn build(
        musicbrainz: MusicBrainzClient,
        wikidata: reqwest::Url,
        commons: reqwest::Url,
        loopback: bool,
    ) -> Self {
        Self {
            musicbrainz,
            client: Default::default(),
            wikidata,
            commons,
            loopback,
            next_start: Arc::new(Mutex::new(None)),
        }
    }

    fn client(&self) -> Result<&reqwest::Client, Error> {
        if let Some(client) = self.client.get() {
            return Ok(client);
        }
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            let _ = rustls::crypto::ring::default_provider().install_default();
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(15))
            .retry(reqwest::retry::never())
            .user_agent(concat!(
                "Hito/",
                env!("CARGO_PKG_VERSION"),
                " (contact@example.invalid)"
            ))
            .build()
            .map_err(|_| Error::Configuration)?;
        Ok(self.client.get_or_init(|| client))
    }

    #[tracing::instrument(level = "debug", skip_all, fields(operation = "artist_image"))]
    pub async fn photo(&self, artist: &str) -> Result<Option<CoverArt>, Error> {
        let started = std::time::Instant::now();
        let result = self.resolve(artist).await;
        tracing::debug!(
            outcome = match &result {
                Ok(Some(_)) => "found",
                Ok(None) => "absent",
                Err(error) => error.code(),
            },
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Artist photo lookup completed"
        );
        result
    }

    async fn resolve(&self, artist: &str) -> Result<Option<CoverArt>, Error> {
        let Some(entity) = self.musicbrainz.artist_wikidata(artist).await? else {
            return Ok(None);
        };
        let Some(file) = self.image_file(&entity).await? else {
            return Ok(None);
        };
        let mut url = self
            .commons
            .join(&format!(
                "wiki/Special:FilePath/{}",
                encode_file_name(&file)
            ))
            .map_err(|_| Error::Configuration)?;
        url.query_pairs_mut().append_pair("width", THUMBNAIL_WIDTH);
        let response = self.follow(url, "image/jpeg, image/png").await?;
        let image = read_image(response).await?;
        match image {
            Some(image) if has_signature(&image) => Ok(Some(image)),
            Some(_) => Err(Error::InvalidResponse),
            None => Ok(None),
        }
    }

    async fn image_file(&self, entity: &str) -> Result<Option<String>, Error> {
        if !identifiers::valid_entity(entity) {
            return Err(Error::InvalidLookup);
        }
        let url = self
            .wikidata
            .join(&format!("wiki/Special:EntityData/{entity}.json"))
            .map_err(|_| Error::Configuration)?;
        let mut response = self.follow(url, "application/json").await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !response.status().is_success() {
            return Err(Error::Status(response.status().as_u16()));
        }
        if response
            .headers()
            .get("content-encoding")
            .is_some_and(|value| value != "identity")
        {
            return Err(Error::InvalidResponse);
        }
        let json = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"));
        if !json {
            return Err(Error::InvalidResponse);
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_ENTITY_BYTES as u64)
        {
            return Err(Error::ResponseTooLarge);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Transport)? {
            if chunk.len() > MAX_ENTITY_BYTES - bytes.len() {
                return Err(Error::ResponseTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        let document: Value = serde_json::from_slice(&bytes).map_err(|_| Error::InvalidLookup)?;
        Ok(image_name(&document, entity))
    }

    async fn follow(
        &self,
        mut url: reqwest::Url,
        accept: &str,
    ) -> Result<reqwest::Response, Error> {
        for _ in 0..=MAX_REDIRECTS {
            self.wait_for_start().await;
            let response = self
                .client()?
                .get(url.clone())
                .header("accept", accept)
                .header("accept-encoding", "identity")
                .send()
                .await
                .map_err(|_| Error::Transport)?;
            if !response.status().is_redirection() {
                return Ok(response);
            }
            url = response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| url.join(value).ok())
                .filter(|next| self.permitted(next))
                .ok_or(Error::InvalidResponse)?;
        }
        Err(Error::InvalidResponse)
    }

    fn permitted(&self, url: &reqwest::Url) -> bool {
        if self.loopback {
            return url.scheme() == "http"
                && url.host_str() == self.commons.host_str()
                && url.port() == self.commons.port();
        }
        url.scheme() == "https"
            && url.port().is_none()
            && url.username().is_empty()
            && url.password().is_none()
            && url
                .host_str()
                .is_some_and(|host| ALLOWED_HOSTS.contains(&host))
    }

    async fn wait_for_start(&self) {
        let mut next_start = self.next_start.lock().await;
        if let Some(next_start) = *next_start {
            tokio::time::sleep_until(next_start).await;
        }
        *next_start = Some(Instant::now() + REQUEST_SPACING);
    }
}

fn image_name(document: &Value, entity: &str) -> Option<String> {
    let claims = document
        .get("entities")?
        .get(entity)?
        .get("claims")?
        .get("P18")?
        .as_array()?;
    let named = |claim: &&Value| -> Option<String> {
        let name = claim
            .get("mainsnak")?
            .get("datavalue")?
            .get("value")?
            .as_str()?;
        valid_file_name(name).then(|| name.to_owned())
    };
    let preferred = claims
        .iter()
        .filter(|claim| claim.get("rank").and_then(Value::as_str) == Some("preferred"))
        .find_map(|claim| named(&claim));
    preferred.or_else(|| {
        claims
            .iter()
            .filter(|claim| claim.get("rank").and_then(Value::as_str) != Some("deprecated"))
            .find_map(|claim| named(&claim))
    })
}

fn valid_file_name(name: &str) -> bool {
    !name.trim().is_empty()
        && name.len() <= MAX_FILE_NAME_BYTES
        && !name
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\' | '#' | '?'))
}

fn encode_file_name(name: &str) -> String {
    let mut encoded = String::with_capacity(name.len() * 3);
    for byte in name.replace(' ', "_").bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn has_signature(image: &CoverArt) -> bool {
    match image.content_type {
        "image/png" => image.bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => image.bytes.starts_with(&[0xff, 0xd8, 0xff]),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ENTITY: &str = "Q42";

    fn claim(name: &str, rank: &str) -> Value {
        json!({"rank": rank, "mainsnak": {"datavalue": {"value": name}}})
    }

    fn document(claims: Vec<Value>) -> Value {
        json!({"entities": {ENTITY: {"claims": {"P18": claims}}}})
    }

    #[test]
    fn the_preferred_image_wins_and_deprecated_or_unsafe_names_are_skipped() {
        let found = image_name(
            &document(vec![
                claim("Old.jpg", "deprecated"),
                claim("Normal.jpg", "normal"),
                claim("Best.png", "preferred"),
            ]),
            ENTITY,
        );
        assert_eq!(found.as_deref(), Some("Best.png"));
        let found = image_name(
            &document(vec![
                claim("Old.jpg", "deprecated"),
                claim("../x.png", "normal"),
                claim("Fine one.jpg", "normal"),
            ]),
            ENTITY,
        );
        assert_eq!(found.as_deref(), Some("Fine one.jpg"));
        assert_eq!(image_name(&document(vec![]), ENTITY), None);
    }

    #[test]
    fn file_names_are_encoded_as_one_path_segment() {
        assert_eq!(
            encode_file_name("Adele at the 2016 Brit Awards (é).jpg"),
            "Adele_at_the_2016_Brit_Awards_%28%C3%A9%29.jpg"
        );
        assert!(!valid_file_name("a/b.jpg"));
        assert!(!valid_file_name("a?b.jpg"));
        assert!(!valid_file_name(&"x".repeat(241)));
    }
}
