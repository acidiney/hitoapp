use super::{
    identity_authority::{RecordingTarget, workflow},
    identity_types::*,
};
use crate::{Actor, Catalog, Error, Result};
use hito_plugin_sdk::{Capability, Manifest, SearchPage, SearchRequest};
use std::time::Duration;

pub(super) const REQUIRED: &[Capability] =
    &[Capability::Search, Capability::Metadata, Capability::Lyrics];
const MAX_MANIFEST_BYTES: i64 = 65_536;

#[derive(sqlx::FromRow)]
struct Installation {
    installation_id: String,
    installation_revision: i64,
    manifest: Option<String>,
}

fn approved(row: &Installation, capabilities: &[Capability]) -> Result<Option<Manifest>> {
    let manifest: Manifest = row
        .manifest
        .as_deref()
        .and_then(|value| serde_json::from_str(value).ok())
        .filter(|manifest: &Manifest| manifest.validate().is_ok())
        .ok_or_else(|| Error::Internal("Stored lyric service manifest is invalid".into()))?;
    if capabilities
        .iter()
        .all(|capability| manifest.capabilities.contains(capability))
    {
        Ok(Some(manifest))
    } else {
        Ok(None)
    }
}

pub(super) async fn approved_services(
    connection: &mut sqlx::SqliteConnection,
    actor: &Actor,
) -> Result<Vec<RecordingLyricService>> {
    let rows: Vec<Installation> = sqlx::query_as("SELECT installation.id AS installation_id,installation.revision AS installation_revision,
        CASE WHEN length(CAST(installation.manifest AS BLOB))<=? THEN installation.manifest END AS manifest
        FROM plugin_installations installation JOIN plugin_services service ON service.id=installation.service_id AND service.active=1
        WHERE installation.user_id=? AND installation.active=1 ORDER BY installation.id LIMIT 257")
        .bind(MAX_MANIFEST_BYTES)
        .bind(actor.id())
        .fetch_all(&mut *connection)
        .await?;
    if rows.len() > 256 {
        return Err(Error::Conflict("Too many lyric services to list".into()));
    }
    let mut items = Vec::new();
    for row in rows {
        if let Some(manifest) = approved(&row, REQUIRED)? {
            items.push(RecordingLyricService {
                installation_id: row.installation_id,
                installation_revision: row.installation_revision,
                label: manifest.name,
            });
        }
    }
    Ok(items)
}

impl Catalog {
    pub async fn recording_lyric_services(
        &self,
        actor: &Actor,
        recording: &str,
    ) -> Result<RecordingLyricServices> {
        workflow("lyric_services", Duration::from_secs(1), async {
            let mut tx = self.store.pool.begin().await?;
            let target = RecordingTarget::capture(&mut tx, actor, recording).await?;
            let items = approved_services(&mut tx, actor).await?;
            target.recheck(&mut tx, actor, recording).await?;
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            Ok(RecordingLyricServices {
                recording_id: recording.into(),
                items,
            })
        })
        .await
    }

    pub async fn search_recording_lyrics(
        &self,
        actor: &Actor,
        recording: &str,
        request: SearchRecordingLyrics,
    ) -> Result<SearchPage> {
        workflow("lyric_candidates", super::LOOKUP_BUDGET, async {
            let target =
                RecordingTarget::capture(&mut *self.store.pool.acquire().await?, actor, recording)
                    .await?;
            validate_installation(&request.installation_id, request.installation_revision)?;
            let search = SearchRequest {
                query: request.query,
                cursor: None,
                limit: 20,
            };
            search.validate().map_err(|_| {
                Error::Invalid("Use a lyric search between one and 512 characters".into())
            })?;
            let operation = self
                .plugins
                .operation(
                    actor,
                    &request.installation_id,
                    request.installation_revision,
                    REQUIRED,
                )
                .await?;
            self.lyric_exchange(
                actor,
                recording,
                &target,
                &operation,
                operation.lyric_search_checked(&search, || {
                    self.check_lyric_target(actor, recording, &target, &operation)
                }),
            )
            .await
        })
        .await
    }
}

pub(super) fn validate_installation(id: &str, revision: i64) -> Result<()> {
    if !super::valid_id(id) || revision < 1 {
        return Err(Error::Invalid(
            "Use a current lyric service revision".into(),
        ));
    }
    Ok(())
}
