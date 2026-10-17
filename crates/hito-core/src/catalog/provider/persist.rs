use crate::{Actor, Error, HostDeviceId, Recording, Result};
use hito_plugin_sdk::{ProviderRecording, ProviderRelease, SourcePage};
use sqlx::{QueryBuilder, Sqlite, Transaction};
use uuid::Uuid;

pub(super) enum ReleasePublication {
    PreserveExisting,
    PublishInherited,
}

pub(super) async fn release(
    tx: &mut Transaction<'_, Sqlite>,
    library: &str,
    installation: &str,
    details: Option<&ProviderRelease>,
    publication: ReleasePublication,
) -> Result<Option<String>> {
    let Some(details) = details else {
        return Ok(None);
    };
    let key = serde_json::to_string(&("plugin", installation, &details.id))
        .map_err(|_| Error::Internal("Provider release identity could not be encoded".into()))?;
    sqlx::query("INSERT INTO releases(id,library_id,title,artist,identity_key) VALUES (?,?,?,?,?) ON CONFLICT(library_id,identity_key) DO UPDATE SET title = excluded.title,artist = excluded.artist WHERE ?")
        .bind(Uuid::new_v4().to_string()).bind(library).bind(&details.title).bind(&details.artist).bind(&key)
        .bind(matches!(publication, ReleasePublication::PublishInherited)).execute(&mut **tx).await?;
    Ok(
        sqlx::query_scalar("SELECT id FROM releases WHERE library_id = ? AND identity_key = ?")
            .bind(library)
            .bind(key)
            .fetch_optional(&mut **tx)
            .await?,
    )
}

pub(super) async fn sources(
    tx: &mut Transaction<'_, Sqlite>,
    binding: &super::bindings::Binding,
    release: Option<&str>,
    observation: &str,
    details: &ProviderRecording,
    page: &SourcePage,
) -> Result<()> {
    let generation = binding.enroll_sources(tx, observation).await?;
    let ids = super::sources::replace(
        tx,
        binding,
        page,
        super::sources::Snapshot {
            generation,
            observation,
        },
    )
    .await?;
    for id in ids {
        sqlx::query("INSERT INTO source_metadata(source_id,release_id,title,artist,duration_ms,disc_number,track_number,inspection_version,observation_id,provenance) VALUES (?,?,?,?,?,?,?,0,?,'provider_response')
            ON CONFLICT(source_id) DO UPDATE SET release_id = excluded.release_id,title = excluded.title,artist = excluded.artist,duration_ms = excluded.duration_ms,disc_number = excluded.disc_number,track_number = excluded.track_number,observation_id = excluded.observation_id,provenance = excluded.provenance")
            .bind(&id).bind(release).bind(&details.title).bind(&details.artist).bind(details.duration_ms.unwrap_or(0) as i64)
            .bind(details.disc_number.map(i64::from)).bind(details.track_number.map(i64::from)).bind(observation).execute(&mut **tx).await?;
        super::sources::project(tx, &id).await?;
    }
    Ok(())
}

pub(super) async fn selected(
    tx: &mut Transaction<'_, Sqlite>,
    actor: &Actor,
    host: &HostDeviceId,
    recording: &str,
) -> Result<Recording> {
    let mut query = QueryBuilder::new(
        "SELECT r.id,r.library_id,r.release_id,COALESCE(p.override_title,r.title) AS title,r.title AS inherited_title,COALESCE(p.override_artist,r.artist) AS artist,r.artist AS inherited_artist,a.title AS album,r.duration_ms,",
    );
    crate::catalog::push_available(&mut query, host, actor.id());
    query.push(" AS available,COALESCE(k.liked,0) AS liked FROM recordings r LEFT JOIN personal_recording_metadata p ON p.recording_id = r.id AND p.user_id = ").push_bind(actor.id().to_owned())
        .push(" LEFT JOIN account_releases a ON a.id = r.release_id AND a.user_id = ").push_bind(actor.id().to_owned())
        .push(" LEFT JOIN likes k ON k.recording_id = r.id AND k.user_id = ").push_bind(actor.id().to_owned())
        .push(" WHERE r.id = ").push_bind(recording.to_owned());
    Ok(query.build_query_as().fetch_one(&mut **tx).await?)
}
