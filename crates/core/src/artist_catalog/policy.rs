use super::{
    MetadataProvider, MetadataSettings, REQUEST_BUDGET, UpdateAutomaticLookups,
    UpdateMetadataSettings,
};
use crate::{Actor, ArtistCatalog, Error, Result};
use sqlx::{Row, SqliteConnection};
use std::{collections::HashSet, time::Instant};
use uuid::Uuid;

const MAX_PROVIDERS: usize = 8;

impl ArtistCatalog {
    #[tracing::instrument(level = "debug", skip_all, fields(operation = "metadata_settings", request_id = %Uuid::new_v4()))]
    pub async fn metadata_settings(&self, actor: &Actor) -> Result<MetadataSettings> {
        let started = Instant::now();
        let result = tokio::time::timeout(REQUEST_BUDGET, async {
            let mut tx = self.store.pool.begin().await?;
            actor.check_session_on(&mut tx).await?;
            let settings = read(&mut tx, actor).await?;
            tx.commit().await?;
            Ok(settings)
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Metadata settings exceeded the time limit",
        )));
        tracing::debug!(
            outcome = result
                .as_ref()
                .map(|_| "success")
                .unwrap_or_else(|error| error.code()),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Metadata settings read completed"
        );
        result
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "metadata_settings_update", request_id = %Uuid::new_v4()))]
    pub async fn set_metadata_settings(
        &self,
        actor: &Actor,
        request: UpdateMetadataSettings,
    ) -> Result<MetadataSettings> {
        let started = Instant::now();
        let result =
            tokio::time::timeout(REQUEST_BUDGET, self.update_metadata_policy(actor, request))
                .await
                .unwrap_or(Err(Error::Upstream(
                    "Metadata settings exceeded the time limit",
                )));
        tracing::info!(
            outcome = result
                .as_ref()
                .map(|_| "success")
                .unwrap_or_else(|error| error.code()),
            provider_count = result
                .as_ref()
                .map(|settings| settings.providers.len())
                .unwrap_or(0),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Metadata settings update completed"
        );
        result
    }

    async fn update_metadata_policy(
        &self,
        actor: &Actor,
        request: UpdateMetadataSettings,
    ) -> Result<MetadataSettings> {
        actor.check_session(&self.store).await?;
        if request.providers.len() > MAX_PROVIDERS
            || (request.enabled && request.providers.is_empty())
        {
            return Err(Error::Invalid(
                "Select between one and eight metadata providers when enrichment is enabled".into(),
            ));
        }
        let mut seen = HashSet::new();
        for provider in &request.providers {
            provider.validate()?;
            if !seen.insert(provider) {
                return Err(Error::Invalid(
                    "Metadata providers must not be repeated".into(),
                ));
            }
        }
        let providers = serde_json::to_string(&request.providers)
            .map_err(|_| Error::Internal("Metadata settings could not be encoded".into()))?;
        if providers.len() > 4096 {
            return Err(Error::Invalid(
                "Metadata settings exceed the size limit".into(),
            ));
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let current = read(&mut tx, actor).await?;
        if current.revision != request.revision {
            return Err(changed());
        }
        if request.enabled {
            for provider in &request.providers {
                approved(&mut tx, actor, provider).await?;
            }
        }
        let revision = current
            .revision
            .checked_add(1)
            .ok_or_else(|| Error::Conflict("Metadata settings cannot be changed further".into()))?;
        sqlx::query("INSERT INTO account_metadata_policies(user_id,revision,enabled,providers) VALUES (?,?,?,?)
            ON CONFLICT(user_id) DO UPDATE SET revision=excluded.revision,enabled=excluded.enabled,providers=excluded.providers")
            .bind(actor.id()).bind(revision).bind(request.enabled).bind(providers)
            .execute(&mut *tx).await?;
        let (automatic_artist_lookup, automatic_artwork) = automation(&mut tx, actor).await?;
        tx.commit().await?;
        Ok(MetadataSettings {
            revision,
            enabled: request.enabled,
            providers: request.providers,
            automatic_artist_lookup,
            automatic_artwork,
        })
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "metadata_automation_update", request_id = %Uuid::new_v4()))]
    pub async fn set_automatic_lookups(
        &self,
        actor: &Actor,
        request: UpdateAutomaticLookups,
    ) -> Result<MetadataSettings> {
        let started = Instant::now();
        let result: Result<MetadataSettings> = async {
            let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
            actor.check_session_on(&mut tx).await?;
            let (artist_lookup, artwork) = automation(&mut tx, actor).await?;
            sqlx::query("INSERT INTO account_metadata_automation(user_id,artist_lookup,artwork) VALUES (?,?,?)
                ON CONFLICT(user_id) DO UPDATE SET artist_lookup=excluded.artist_lookup,artwork=excluded.artwork")
                .bind(actor.id())
                .bind(request.automatic_artist_lookup.unwrap_or(artist_lookup))
                .bind(request.automatic_artwork.unwrap_or(artwork))
                .execute(&mut *tx)
                .await?;
            let settings = read(&mut tx, actor).await?;
            tx.commit().await?;
            Ok(settings)
        }
        .await;
        tracing::info!(
            outcome = result
                .as_ref()
                .map(|_| "success")
                .unwrap_or_else(|error| error.code()),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Metadata automation update completed"
        );
        result
    }
}

async fn automation(connection: &mut SqliteConnection, actor: &Actor) -> Result<(bool, bool)> {
    let stored: Option<(bool, bool)> = sqlx::query_as(
        "SELECT artist_lookup,artwork FROM account_metadata_automation WHERE user_id=?",
    )
    .bind(actor.id())
    .fetch_optional(connection)
    .await?;
    Ok(stored.unwrap_or((true, true)))
}

pub(super) async fn read(
    connection: &mut SqliteConnection,
    actor: &Actor,
) -> Result<MetadataSettings> {
    let row = sqlx::query(
        "SELECT revision,enabled,providers FROM account_metadata_policies WHERE user_id=?",
    )
    .bind(actor.id())
    .fetch_optional(&mut *connection)
    .await?;
    let (automatic_artist_lookup, automatic_artwork) = automation(&mut *connection, actor).await?;
    let Some(row) = row else {
        return Ok(MetadataSettings {
            revision: 0,
            enabled: true,
            providers: vec![MetadataProvider::BuiltinMusicBrainz],
            automatic_artist_lookup,
            automatic_artwork,
        });
    };
    let providers: Vec<MetadataProvider> = serde_json::from_str(row.try_get("providers")?)
        .map_err(|_| Error::Internal("Stored metadata settings are invalid".into()))?;
    if providers.len() > MAX_PROVIDERS {
        return Err(Error::Internal(
            "Stored metadata settings are invalid".into(),
        ));
    }
    Ok(MetadataSettings {
        revision: row.try_get("revision")?,
        enabled: row.try_get("enabled")?,
        providers,
        automatic_artist_lookup,
        automatic_artwork,
    })
}

pub(super) async fn approved(
    connection: &mut SqliteConnection,
    actor: &Actor,
    provider: &MetadataProvider,
) -> Result<()> {
    let MetadataProvider::HttpPlugin { installation_id } = provider else {
        return Ok(());
    };
    let approved: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM plugin_installations p
        JOIN plugin_services service ON service.id=p.service_id AND service.active=1
        WHERE p.id=? AND p.user_id=? AND p.active=1
            AND EXISTS(SELECT 1 FROM json_each(p.manifest,'$.capabilities') WHERE value='catalog'))")
        .bind(installation_id).bind(actor.id()).fetch_one(connection).await?;
    if approved {
        Ok(())
    } else {
        Err(Error::NotFound)
    }
}

pub(super) fn selected(settings: &MetadataSettings, provider: &MetadataProvider) -> bool {
    settings.enabled && settings.providers.contains(provider)
}

pub(super) fn changed() -> Error {
    Error::Conflict("Metadata settings changed. Review them again.".into())
}

impl MetadataProvider {
    pub(super) fn validate(&self) -> Result<()> {
        if let Self::HttpPlugin { installation_id } = self
            && (installation_id.len() > 64 || Uuid::parse_str(installation_id).is_err())
        {
            return Err(Error::Invalid(
                "Use a valid metadata provider connection".into(),
            ));
        }
        Ok(())
    }
}
