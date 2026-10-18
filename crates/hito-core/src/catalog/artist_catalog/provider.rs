use super::MetadataProvider;
use crate::{Actor, Catalog, Error, Result, plugins::Operation};
use hito_metadata_musicbrainz::MusicBrainzClient;
use hito_plugin_sdk::{ArtistProfileRequest, Capability, CatalogArtistProfile};
use sqlx::SqliteConnection;
use uuid::Uuid;

pub(super) enum Connection {
    Builtin(MusicBrainzClient),
    Http(Box<Operation>),
}

impl Connection {
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
                let revision: i64 = sqlx::query_scalar("SELECT installation.revision FROM plugin_installations installation
                    JOIN plugin_services service ON service.id=installation.service_id AND service.active=1
                    WHERE installation.id=? AND installation.user_id=? AND installation.active=1
                        AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value='catalog')")
                    .bind(installation_id).bind(actor.id()).fetch_optional(&catalog.store.pool).await?.ok_or(Error::NotFound)?;
                let operation = catalog
                    .plugins
                    .operation(actor, installation_id, revision, &[Capability::Catalog])
                    .await?;
                Ok(Self::Http(Box::new(operation)))
            }
        }
    }

    pub(super) async fn supported(&self) -> Result<bool> {
        match self {
            Self::Builtin(_) => Ok(true),
            Self::Http(operation) => operation.catalog_profile_supported().await,
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
