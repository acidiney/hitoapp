use super::Catalog;
use crate::{
    Actor, Error, Result,
    library_storage::{ManagedFileLocation, managed_mime},
};
use uuid::Uuid;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct ManagedLocation {
    pub file: ManagedFileLocation,
    intent: String,
    generation: i64,
    access_grant: String,
}

#[derive(sqlx::FromRow)]
struct Selected {
    id: String,
    attempt_id: String,
    generation: i64,
    checksum: String,
    byte_length: i64,
    mime: String,
    intent: String,
    intent_generation: i64,
    access_grant: String,
}

impl Catalog {
    pub(super) async fn managed_location(
        &self,
        actor: &Actor,
        source: &str,
    ) -> Result<Option<ManagedLocation>> {
        let row: Option<Selected> = sqlx::query_as(
            "SELECT file.id,file.attempt_id,file.generation,file.checksum,file.byte_length,file.mime,
                intent.id AS intent,intent.generation AS intent_generation,intent.access_grant_id AS access_grant
             FROM sources s JOIN recordings recording ON recording.id = s.recording_id
             JOIN download_files file ON file.source_id = s.id AND file.recording_id = recording.id
             JOIN download_intents intent ON intent.file_id = file.id AND intent.account_id = file.account_id
                AND intent.host_device = file.host_device AND intent.recording_id = file.recording_id
                AND intent.library_id = recording.library_id AND intent.state = 'ready'
             JOIN library_access_grants access ON access.library_id = intent.library_id
                AND access.user_id = intent.account_id AND access.id = intent.access_grant_id
             WHERE s.id = ? AND s.provider = 'download' AND s.ready = 1 AND s.device_id = ?
                AND file.host_device = ? AND file.account_id = ? AND file.state = 'ready'
             ORDER BY intent.id LIMIT 1",
        ).bind(source).bind(self.host.as_str()).bind(self.host.as_str()).bind(actor.id())
            .fetch_optional(&self.store.pool).await?;
        let Some(row) = row else {
            return Ok(None);
        };
        Ok(Some(ManagedLocation {
            file: ManagedFileLocation {
                file_id: Uuid::parse_str(&row.id).map_err(|_| Error::NotFound)?,
                attempt_id: Uuid::parse_str(&row.attempt_id).map_err(|_| Error::NotFound)?,
                generation: row.generation,
                checksum: row.checksum,
                byte_length: u64::try_from(row.byte_length).map_err(|_| Error::NotFound)?,
                mime: managed_mime(&row.mime).ok_or(Error::NotFound)?,
            },
            intent: row.intent,
            generation: row.intent_generation,
            access_grant: row.access_grant,
        }))
    }
}
