use crate::{
    ArtistRecordings, Error, GenreEntity, IsrcRecording, NameSearch, RecordingProbe, discography,
    edition, edition_listing, identifiers, projection, search, transport::Transport,
};
use hito_plugin_sdk::{
    ArtistDiscographyPage, ArtistDiscographyRequest, ArtistProfileRequest, ArtistSearchPage,
    ArtistSearchRequest, CatalogArtistProfile, CatalogEditionTrackList, EditionTrackListRequest,
    ReleaseGroupEditionsPage, ReleaseGroupEditionsRequest,
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
        Self::with_loopback_endpoint(endpoint)
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn with_loopback_endpoint(endpoint: reqwest::Url) -> Result<Self, Error> {
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
                .release_groups(
                    &request.artist_id,
                    request.release_type.map(discography::type_parameter),
                    request.limit,
                    offset,
                )
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

    #[tracing::instrument(level = "debug", skip_all, fields(operation = "release_group_editions", request_id = tracing::field::Empty))]
    pub async fn release_group_editions(
        &self,
        request: ReleaseGroupEditionsRequest,
    ) -> Result<ReleaseGroupEditionsPage, Error> {
        let started = Instant::now();
        let request_id = Uuid::new_v4();
        tracing::Span::current().record("request_id", tracing::field::display(request_id));
        let result = async {
            let offset = edition_listing::request_offset(&request)?;
            let bytes = self
                .transport
                .editions(&request.release_group_id, request.limit, offset)
                .await
                .map_err(|error| match error {
                    Error::InvalidResponse => Error::InvalidEdition,
                    error => error,
                })?;
            edition_listing::page(&bytes, &request, offset)
        }
        .await;
        tracing::debug!(
            outcome = match &result {
                Ok(_) => "success",
                Err(error) => error.code(),
            },
            item_count = result.as_ref().map(|page| page.items.len()).unwrap_or(0),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "MusicBrainz release-group editions completed"
        );
        result
    }

    #[tracing::instrument(level = "debug", skip_all, fields(operation = "edition_track_list", request_id = tracing::field::Empty))]
    pub async fn edition_track_list(
        &self,
        request: EditionTrackListRequest,
    ) -> Result<CatalogEditionTrackList, Error> {
        let started = Instant::now();
        let request_id = Uuid::new_v4();
        tracing::Span::current().record("request_id", tracing::field::display(request_id));
        let result = async {
            edition::validate_request(&request)?;
            let bytes = self
                .transport
                .edition(&request.edition_id)
                .await
                .map_err(|error| match error {
                    Error::InvalidResponse => Error::InvalidEdition,
                    error => error,
                })?;
            edition::track_list(&bytes, &request)
        }
        .await;
        tracing::debug!(
            outcome = match &result {
                Ok(_) => "success",
                Err(error) => error.code(),
            },
            medium_count = result
                .as_ref()
                .map(|edition| edition.media.len())
                .unwrap_or(0),
            track_count = result
                .as_ref()
                .map(|edition| edition
                    .media
                    .iter()
                    .map(|medium| medium.tracks.len())
                    .sum::<usize>())
                .unwrap_or(0),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "MusicBrainz edition track list completed"
        );
        result
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "isrc_lookup", request_id = tracing::field::Empty))]
    pub async fn recordings_by_isrc(&self, isrc: &str) -> Result<Vec<IsrcRecording>, Error> {
        let started = Instant::now();
        let request_id = Uuid::new_v4();
        tracing::Span::current().record("request_id", tracing::field::display(request_id));
        let result = async {
            if !identifiers::valid_isrc(isrc) {
                return Err(Error::InvalidLookupRequest);
            }
            match self.transport.isrc(isrc).await {
                Ok(bytes) => identifiers::isrc_recordings(&bytes, isrc),
                Err(Error::Status(404)) => Ok(Vec::new()),
                Err(Error::InvalidResponse) => Err(Error::InvalidLookup),
                Err(error) => Err(error),
            }
        }
        .await;
        tracing::info!(
            outcome = match &result {
                Ok(_) => "success",
                Err(error) => error.code(),
            },
            item_count = result.as_ref().map(Vec::len).unwrap_or(0),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "MusicBrainz ISRC lookup completed"
        );
        result
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "recording_lookup", request_id = tracing::field::Empty))]
    pub async fn recording_artists(&self, id: &str) -> Result<Option<IsrcRecording>, Error> {
        let started = Instant::now();
        let request_id = Uuid::new_v4();
        tracing::Span::current().record("request_id", tracing::field::display(request_id));
        let result = async {
            if !identifiers::valid_recording(id) {
                return Err(Error::InvalidLookupRequest);
            }
            match self.transport.recording_credits(id).await {
                Ok(bytes) => identifiers::credited_recording(&bytes).map(Some),
                Err(Error::Status(404)) => Ok(None),
                Err(Error::InvalidResponse) => Err(Error::InvalidLookup),
                Err(error) => Err(error),
            }
        }
        .await;
        tracing::info!(
            outcome = match &result {
                Ok(_) => "success",
                Err(error) => error.code(),
            },
            elapsed_ms = started.elapsed().as_millis() as u64,
            "MusicBrainz recording lookup completed"
        );
        result
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "artist_recordings", request_id = tracing::field::Empty))]
    pub async fn artist_recordings_matching(
        &self,
        artist_id: &str,
        probes: &[RecordingProbe],
    ) -> Result<ArtistRecordings, Error> {
        let started = Instant::now();
        let request_id = Uuid::new_v4();
        tracing::Span::current().record("request_id", tracing::field::display(request_id));
        let result = async {
            let query = identifiers::probe_query(artist_id, probes)?;
            match self.transport.recording_search(&query).await {
                Ok(bytes) => identifiers::matching_recordings(&bytes),
                Err(Error::Status(404)) => Ok(ArtistRecordings {
                    total: 0,
                    items: Vec::new(),
                }),
                Err(Error::InvalidResponse) => Err(Error::InvalidLookup),
                Err(error) => Err(error),
            }
        }
        .await;
        tracing::info!(
            outcome = match &result {
                Ok(_) => "success",
                Err(error) => error.code(),
            },
            probes = probes.len(),
            item_count = result.as_ref().map(|found| found.items.len()).unwrap_or(0),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "MusicBrainz artist recordings completed"
        );
        result
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "genre_lookup", request_id = tracing::field::Empty))]
    pub async fn genres(&self, entity: GenreEntity, id: &str) -> Result<Vec<String>, Error> {
        let started = Instant::now();
        let request_id = Uuid::new_v4();
        tracing::Span::current().record("request_id", tracing::field::display(request_id));
        let result = async {
            if !identifiers::canonical_artist(id) {
                return Err(Error::InvalidLookupRequest);
            }
            match self.transport.genres(entity.path(), id).await {
                Ok(bytes) => identifiers::genre_names(&bytes, id),
                Err(Error::Status(404)) => Ok(Vec::new()),
                Err(Error::InvalidResponse) => Err(Error::InvalidLookup),
                Err(error) => Err(error),
            }
        }
        .await;
        tracing::info!(
            outcome = match &result {
                Ok(_) => "success",
                Err(error) => error.code(),
            },
            item_count = result.as_ref().map(Vec::len).unwrap_or(0),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "MusicBrainz genre lookup completed"
        );
        result
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "url_lookup", request_id = tracing::field::Empty))]
    pub async fn artists_by_url(&self, resource: &str) -> Result<Vec<String>, Error> {
        let started = Instant::now();
        let request_id = Uuid::new_v4();
        tracing::Span::current().record("request_id", tracing::field::display(request_id));
        let result = async {
            if !identifiers::valid_url(resource) {
                return Err(Error::InvalidLookupRequest);
            }
            match self.transport.url_relations(resource).await {
                Ok(bytes) => identifiers::url_artists(&bytes),
                Err(Error::Status(404)) => Ok(Vec::new()),
                Err(Error::InvalidResponse) => Err(Error::InvalidLookup),
                Err(error) => Err(error),
            }
        }
        .await;
        tracing::info!(
            outcome = match &result {
                Ok(_) => "success",
                Err(error) => error.code(),
            },
            item_count = result.as_ref().map(Vec::len).unwrap_or(0),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "MusicBrainz URL lookup completed"
        );
        result
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "artist_name_search", request_id = tracing::field::Empty))]
    pub async fn artist_name_search(&self, name: &str) -> Result<NameSearch, Error> {
        let started = Instant::now();
        let request_id = Uuid::new_v4();
        tracing::Span::current().record("request_id", tracing::field::display(request_id));
        let result = async {
            if name.trim().is_empty() || name.len() > 512 {
                return Err(Error::InvalidSearchRequest);
            }
            let bytes = self
                .transport
                .artist_name_search(&search::literal_query(name))
                .await
                .map_err(|error| match error {
                    Error::InvalidResponse => Error::InvalidSearch,
                    error => error,
                })?;
            search::name_search(&bytes)
        }
        .await;
        tracing::info!(
            outcome = match &result {
                Ok(_) => "success",
                Err(error) => error.code(),
            },
            item_count = result.as_ref().map(|found| found.items.len()).unwrap_or(0),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "MusicBrainz artist name search completed"
        );
        result
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "artist_search", request_id = tracing::field::Empty))]
    pub async fn artist_search(
        &self,
        request: ArtistSearchRequest,
    ) -> Result<ArtistSearchPage, Error> {
        let started = Instant::now();
        let request_id = Uuid::new_v4();
        tracing::Span::current().record("request_id", tracing::field::display(request_id));
        let result = async {
            let offset = search::request_offset(&request)?;
            let literal = search::literal_query(&request.query);
            let bytes = self
                .transport
                .artist_search(&literal, request.limit, offset)
                .await
                .map_err(|error| match error {
                    Error::InvalidResponse => Error::InvalidSearch,
                    error => error,
                })?;
            search::page(&bytes, &request, offset)
        }
        .await;
        tracing::info!(
            outcome = match &result {
                Ok(_) => "success",
                Err(error) => error.code(),
            },
            item_count = result.as_ref().map(|page| page.items.len()).unwrap_or(0),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "MusicBrainz artist search completed"
        );
        result
    }
}
