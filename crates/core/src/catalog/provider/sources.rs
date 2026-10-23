use super::bindings::Binding;
use crate::{Error, Result};
use hito_plugin_sdk::SourcePage;
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

pub(super) struct Snapshot<'a> {
    pub generation: i64,
    pub observation: &'a str,
}

#[tracing::instrument(level = "debug", skip_all, err(Debug))]
pub(super) async fn replace(
    tx: &mut Transaction<'_, Sqlite>,
    binding: &Binding,
    page: &SourcePage,
    snapshot: Snapshot<'_>,
) -> Result<Vec<String>> {
    binding.recheck(tx).await?;
    let generation: i64 = sqlx::query_scalar("UPDATE provider_source_state SET generation = generation + 1,observation_id = ? WHERE binding_id = ? AND generation = ? RETURNING generation")
        .bind(snapshot.observation).bind(&binding.id).bind(snapshot.generation).fetch_optional(&mut **tx).await?
        .ok_or_else(|| Error::Conflict("Playback sources changed during the request. Try again.".into()))?;
    sqlx::query("UPDATE sources SET ready = 0 WHERE id IN (SELECT source_id FROM provider_sources WHERE binding_id = ? AND generation = ?)")
        .bind(&binding.id).bind(snapshot.generation).execute(&mut **tx).await?;
    let mut ids = Vec::with_capacity(page.items.len());
    for source in &page.items {
        let existing: Option<(String, String)> = sqlx::query_as("SELECT ps.source_id,ps.binding_id FROM provider_sources ps JOIN provider_items item ON item.id = ps.binding_id AND item.retired = 0 WHERE ps.installation_id = ? AND ps.provider_source_id = ?")
            .bind(&binding.installation_id).bind(&source.id).fetch_optional(&mut **tx).await?;
        let id = match existing {
            Some((id, owner)) if owner == binding.id => id,
            Some(_) => {
                return Err(Error::Upstream(
                    "The provider reused a source for another recording binding",
                ));
            }
            None => Uuid::new_v4().to_string(),
        };
        let provider_id = serde_json::to_string(&(&binding.id, &source.id))
            .map_err(|_| Error::Internal("Provider source identity could not be encoded".into()))?;
        sqlx::query("INSERT INTO sources(id,recording_id,provider,provider_id,ready) VALUES (?,?,'plugin',?,?) ON CONFLICT(id) DO UPDATE SET ready = excluded.ready")
            .bind(&id).bind(&binding.recording_id).bind(provider_id).bind(source.ready).execute(&mut **tx).await?;
        sqlx::query("INSERT INTO provider_sources(source_id,binding_id,installation_id,recording_id,provider_source_id,label,generation) VALUES (?,?,?,?,?,?,?) ON CONFLICT(source_id) DO UPDATE SET label = excluded.label,generation = excluded.generation")
            .bind(&id).bind(&binding.id).bind(&binding.installation_id).bind(&binding.recording_id).bind(&source.id).bind(&source.label).bind(generation).execute(&mut **tx).await?;
        ids.push(id);
    }
    tracing::debug!(count = ids.len(), "Provider source bindings replaced");
    Ok(ids)
}

pub(super) async fn attach(
    tx: &mut Transaction<'_, Sqlite>,
    source: &str,
    observation: &str,
) -> Result<()> {
    sqlx::query("INSERT INTO source_metadata(source_id,release_id,title,artist,duration_ms,disc_number,track_number,inspection_version,observation_id,provenance)
        SELECT ps.source_id,o.release_id,o.title,o.artist,o.duration_ms,o.disc_number,o.track_number,0,o.id,'provider_response'
        FROM provider_sources ps JOIN metadata_observations o ON o.id = ? AND o.recording_id = ps.recording_id AND o.provider_binding_id = ps.binding_id
        WHERE ps.source_id = ? ON CONFLICT(source_id) DO NOTHING")
        .bind(observation).bind(source).execute(&mut **tx).await?;
    project(tx, source).await
}

pub(super) async fn project(tx: &mut Transaction<'_, Sqlite>, source: &str) -> Result<()> {
    sqlx::query("INSERT INTO source_artist_credits(source_id,artist_id)
        SELECT m.source_id,a.id FROM source_metadata m JOIN sources s ON s.id = m.source_id
        JOIN recordings r ON r.id = s.recording_id JOIN artists a ON a.library_id = r.library_id AND a.name = m.artist COLLATE BINARY
        WHERE m.source_id = ? ON CONFLICT(source_id) DO UPDATE SET artist_id = excluded.artist_id")
        .bind(source).execute(&mut **tx).await?;
    crate::metadata::refresh_source(tx, source).await
}
