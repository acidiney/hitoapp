use crate::{Error, discography, projection, transport::Transport};
use hito_plugin_sdk::{
    ArtistDiscographyPage, ArtistDiscographyRequest, ArtistProfileRequest, CatalogArtistProfile,
};
use std::time::Instant;
use uuid::Uuid;

const MUSICBRAINZ_ROOT: &str = "https://musicbrainz.org/ws/2/";

#[derive(Clone)]
pub struct MusicBrainzClient {
    transport: Transport,
}

impl MusicBrainzClient {
    pub fn new() -> Result<Self, Error> {
        let endpoint = MUSICBRAINZ_ROOT.parse().map_err(|_| Error::Configuration)?;
        Ok(Self {
            transport: Transport::new(endpoint)?,
        })
    }

    #[cfg(test)]
    pub(crate) fn with_test_endpoint(endpoint: reqwest::Url) -> Result<Self, Error> {
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
        Ok(Self {
            transport: Transport::new(endpoint)?,
        })
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "artist_profile", request_id = tracing::field::Empty))]
    pub async fn artist_profile(
        &self,
        request: ArtistProfileRequest,
    ) -> Result<CatalogArtistProfile, Error> {
        let started = Instant::now();
        let request_id = Uuid::new_v4();
        tracing::Span::current().record("request_id", tracing::field::display(request_id));
        let result = async {
            request.validate().map_err(|_| Error::InvalidRequest)?;
            let id = Uuid::parse_str(&request.artist_id).map_err(|_| Error::InvalidRequest)?;
            if id.hyphenated().to_string() != request.artist_id {
                return Err(Error::InvalidRequest);
            }
            let bytes = self.transport.artist(&request.artist_id).await?;
            projection::artist(&bytes, &request)
        }
        .await;
        tracing::info!(
            outcome = match &result {
                Ok(_) => "success",
                Err(error) => error.code(),
            },
            elapsed_ms = started.elapsed().as_millis() as u64,
            "MusicBrainz artist profile completed"
        );
        result
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "artist_discography", request_id = tracing::field::Empty))]
    pub async fn artist_discography(
        &self,
        request: ArtistDiscographyRequest,
    ) -> Result<ArtistDiscographyPage, Error> {
        let started = Instant::now();
        let request_id = Uuid::new_v4();
        tracing::Span::current().record("request_id", tracing::field::display(request_id));
        let result = async {
            let offset = discography::request_offset(&request)?;
            let bytes = self
                .transport
                .release_groups(&request.artist_id, request.limit, offset)
                .await
                .map_err(|error| match error {
                    Error::InvalidResponse => Error::InvalidDiscography,
                    error => error,
                })?;
            discography::page(&bytes, &request, offset)
        }
        .await;
        tracing::info!(
            outcome = match &result {
                Ok(_) => "success",
                Err(error) => error.code(),
            },
            item_count = result.as_ref().map(|page| page.items.len()).unwrap_or(0),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "MusicBrainz artist discography completed"
        );
        result
    }
}
