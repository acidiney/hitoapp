use super::matching::MusicalRecording;
use crate::{Actor, Error, Result, identity::recording_access};
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqliteConnection};

#[derive(Serialize)]
pub(super) struct Target {
    pub library_id: String,
    pub access_grant: String,
    pub binding_id: String,
    pub binding_revision: i64,
    pub observation_id: String,
    pub choice_revision: i64,
    pub musical: MusicalRecording,
}

impl Target {
    pub(super) fn digest(&self) -> Result<String> {
        digest(self)
    }
}

pub(super) fn digest(value: &impl Serialize) -> Result<String> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| Error::Internal("Source resolution evidence could not be encoded".into()))?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub(super) async fn access(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
) -> Result<()> {
    actor.check_session_on(connection).await?;
    recording_access(connection, actor, recording).await?;
    let owner: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM recordings recording JOIN libraries library ON library.id = recording.library_id WHERE recording.id = ? AND library.owner_id = ?)")
        .bind(recording).bind(actor.id()).fetch_one(connection).await?;
    if !owner {
        return Err(Error::NotFound);
    }
    Ok(())
}

pub(super) async fn check(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: Option<&str>,
) -> Result<()> {
    match recording {
        Some(recording) => access(connection, actor, recording).await,
        None => actor.check_session_on(connection).await,
    }
}

pub(super) async fn capture(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
) -> Result<Target> {
    access(connection, actor, recording).await?;
    let row = sqlx::query("SELECT recording.library_id,access.id AS access_grant,item.id AS binding_id,item.revision AS binding_revision,observation.id AS observation_id,
        observation.title,observation.artist,observation.duration_ms,observation.album,COALESCE(choice.revision,0) AS choice_revision
        FROM recordings recording JOIN libraries library ON library.id = recording.library_id AND library.owner_id = ?
        JOIN library_access_grants access ON access.library_id = library.id AND access.user_id = ?
        JOIN provider_items item ON item.id = recording.metadata_binding_id AND item.recording_id = recording.id AND item.role = 'primary' AND item.retired = 0
        JOIN metadata_observations observation ON observation.id = recording.metadata_observation_id AND observation.recording_id = recording.id AND observation.provider_binding_id = item.id
        LEFT JOIN recording_source_choices choice ON choice.user_id = ? AND choice.recording_id = recording.id
        WHERE recording.id = ?")
        .bind(actor.id()).bind(actor.id()).bind(actor.id()).bind(recording).fetch_optional(connection).await?.ok_or(Error::NotFound)?;
    Ok(Target {
        library_id: row.try_get("library_id")?,
        access_grant: row.try_get("access_grant")?,
        binding_id: row.try_get("binding_id")?,
        binding_revision: row.try_get("binding_revision")?,
        observation_id: row.try_get("observation_id")?,
        choice_revision: row.try_get("choice_revision")?,
        musical: MusicalRecording {
            title: row.try_get("title")?,
            artist: row.try_get("artist")?,
            duration_ms: row.try_get("duration_ms")?,
            album: row.try_get("album")?,
        },
    })
}

pub(super) async fn protected(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
) -> Result<bool> {
    Ok(sqlx::query_scalar(
        "SELECT CASE WHEN choice.binding_id IS NOT NULL THEN 1
        WHEN COALESCE(choice.revision,0)>0 THEN 0
        ELSE EXISTS(SELECT 1 FROM provider_items item WHERE item.recording_id=recording.id
            AND item.retired=0 AND item.evidence='user_confirmed') END
        FROM recordings recording LEFT JOIN recording_source_choices choice
            ON choice.user_id=? AND choice.recording_id=recording.id WHERE recording.id=?",
    )
    .bind(actor.id())
    .bind(recording)
    .fetch_one(connection)
    .await?)
}
