use super::{ArtistReviewConnection, MetadataProvider};
use crate::{Actor, Catalog, Error, Result, plugins::Operation};
use hito_metadata_musicbrainz::MusicBrainzClient;
use hito_plugin_sdk::{
    ArtistDiscographyPage, ArtistDiscographyRequest, ArtistProfileRequest, ArtistSearchPage,
    ArtistSearchRequest, Capability, CatalogArtistProfile, CatalogEditionOperation,
    CatalogEditionTrackList, EditionTrackListRequest, ReleaseGroupEditionsPage,
    ReleaseGroupEditionsRequest,
};
use sqlx::SqliteConnection;
use uuid::Uuid;

pub(super) enum Connection {
    Builtin(MusicBrainzClient),
    Http(Box<Operation>),
}

impl Connection {
    pub(super) fn review_connection(&self) -> Option<ArtistReviewConnection> {
        match self {
            Self::Builtin(_) => None,
            Self::Http(operation) => Some(ArtistReviewConnection {
                installation_revision: operation.catalog_installation_revision(),
                service_id: operation.service.clone(),
                service_revision: operation.service_revision,
            }),
        }
    }

    pub(super) async fn open(
        catalog: &Catalog,
        actor: &Actor,
        provider: &MetadataProvider,
    ) -> Result<Self> {
        match provider {
            MetadataProvider::BuiltinMusicBrainz => {
                Ok(Self::Builtin(catalog.artist_catalog.builtin.clone()))
            }
            MetadataProvider::HttpPlugin { installation_id } => {
                let revision = catalog_revision(
                    &mut *catalog.store.pool.acquire().await?,
                    actor,
                    installation_id,
                )
                .await?
                .ok_or(Error::NotFound)?;
                let operation = catalog
                    .plugins
                    .operation(actor, installation_id, revision, &[Capability::Catalog])
                    .await?;
                Ok(Self::Http(Box::new(operation)))
            }
        }
    }

    pub(super) fn saving_revision(&self) -> Option<i64> {
        match self {
            Self::Builtin(_) => None,
            Self::Http(operation) => [
                Capability::Sources,
                Capability::Search,
                Capability::Metadata,
            ]
            .into_iter()
            .any(|capability| operation.offers(capability))
            .then(|| operation.catalog_installation_revision()),
        }
    }

    pub(super) async fn supported(&self) -> Result<bool> {
        match self {
            Self::Builtin(_) => Ok(true),
            Self::Http(operation) => operation.catalog_profile_supported().await,
        }
    }

    pub(super) async fn discovery_supported(&self) -> Result<bool> {
        match self {
            Self::Builtin(_) => Ok(true),
            Self::Http(operation) => operation.catalog_discovery_supported().await,
        }
    }

    pub(super) async fn discography_supported(&self) -> Result<bool> {
        match self {
            Self::Builtin(_) => Ok(true),
            Self::Http(operation) => operation.catalog_discography_supported().await,
        }
    }

    pub(super) async fn edition_supported(
        &self,
        operation: CatalogEditionOperation,
    ) -> Result<bool> {
        match self {
            Self::Builtin(_) => Ok(true),
            Self::Http(connection) => connection.catalog_edition_supported(operation).await,
        }
    }

    pub(super) async fn editions(
        &self,
        request: &ReleaseGroupEditionsRequest,
    ) -> Result<ReleaseGroupEditionsPage> {
        match self {
            Self::Builtin(client) => client
                .release_group_editions(ReleaseGroupEditionsRequest {
                    release_group_id: request.release_group_id.clone(),
                    cursor: request.cursor.clone(),
                    snapshot: request.snapshot.clone(),
                    limit: request.limit,
                })
                .await
                .map_err(|_| {
                    Error::Upstream(
                        "The metadata provider could not return valid release-group editions",
                    )
                }),
            Self::Http(operation) => operation.catalog_release_group_editions(request).await,
        }
    }

    pub(super) async fn edition_tracks(
        &self,
        request: &EditionTrackListRequest,
    ) -> Result<CatalogEditionTrackList> {
        match self {
            Self::Builtin(client) => client
                .edition_track_list(EditionTrackListRequest {
                    edition_id: request.edition_id.clone(),
                    release_group_id: request.release_group_id.clone(),
                })
                .await
                .map_err(|_| {
                    Error::Upstream(
                        "The metadata provider could not return a valid edition track list",
                    )
                }),
            Self::Http(operation) => operation.catalog_edition_track_list(request).await,
        }
    }

    pub(super) async fn search(&self, request: &ArtistSearchRequest) -> Result<ArtistSearchPage> {
        match self {
            Self::Builtin(client) => client
                .artist_search(ArtistSearchRequest {
                    query: request.query.clone(),
                    cursor: request.cursor.clone(),
                    snapshot: request.snapshot.clone(),
                    limit: request.limit,
                })
                .await
                .map_err(|_| {
                    Error::Upstream(
                        "The metadata provider could not return valid artist candidates",
                    )
                }),
            Self::Http(operation) => operation.catalog_artist_search(request).await,
        }
    }

    pub(super) async fn profile(
        &self,
        request: &ArtistProfileRequest,
    ) -> Result<CatalogArtistProfile> {
        match self {
            Self::Builtin(client) => {
                validate_builtin_id(&request.artist_id)?;
                client
                    .artist_profile(ArtistProfileRequest {
                        artist_id: request.artist_id.clone(),
                    })
                    .await
                    .map_err(|_| {
                        Error::Upstream(
                            "The metadata provider could not return a valid artist profile",
                        )
                    })
            }
            Self::Http(operation) => operation.catalog_artist_profile(request).await,
        }
    }

    pub(super) async fn recheck(&self, connection: &mut SqliteConnection) -> Result<()> {
        match self {
            Self::Builtin(_) => Ok(()),
            Self::Http(operation) => operation.recheck(connection).await,
        }
    }

    pub(super) async fn discography(
        &self,
        request: &ArtistDiscographyRequest,
    ) -> Result<ArtistDiscographyPage> {
        match self {
            Self::Builtin(client) => {
                validate_builtin_id(&request.artist_id)?;
                client
                    .artist_discography(ArtistDiscographyRequest {
                        artist_id: request.artist_id.clone(),
                        cursor: request.cursor.clone(),
                        snapshot: request.snapshot.clone(),
                        limit: request.limit,
                    })
                    .await
                    .map_err(|_| {
                        Error::Upstream(
                            "The metadata provider could not return a valid artist discography",
                        )
                    })
            }
            Self::Http(operation) => operation.catalog_artist_discography(request).await,
        }
    }
}

pub(super) async fn catalog_revision(
    connection: &mut SqliteConnection,
    actor: &Actor,
    installation: &str,
) -> Result<Option<i64>> {
    Ok(sqlx::query_scalar("SELECT installation.revision FROM plugin_installations installation
        JOIN plugin_services service ON service.id=installation.service_id AND service.active=1
        WHERE installation.id=? AND installation.user_id=? AND installation.active=1
            AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value='catalog')")
        .bind(installation).bind(actor.id()).fetch_optional(connection).await?)
}

pub(super) fn validate_artist_id(provider: &MetadataProvider, value: &str) -> Result<()> {
    provider.validate()?;
    if value.len() > 256 {
        return Err(Error::Invalid(
            "Use a valid provider artist identifier".into(),
        ));
    }
    ArtistProfileRequest {
        artist_id: value.into(),
    }
    .validate()
    .map_err(|_| Error::Invalid("Use a valid provider artist identifier".into()))?;
    if matches!(provider, MetadataProvider::BuiltinMusicBrainz) {
        validate_builtin_id(value)?;
    }
    Ok(())
}

pub(super) fn validate_builtin_id(value: &str) -> Result<()> {
    let valid =
        Uuid::parse_str(value).is_ok_and(|id| !id.is_nil() && id.hyphenated().to_string() == value);
    if valid {
        Ok(())
    } else {
        Err(Error::Invalid(
            "Use a canonical MusicBrainz artist identifier".into(),
        ))
    }
}
