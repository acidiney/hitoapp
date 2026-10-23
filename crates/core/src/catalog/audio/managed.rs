use super::origin::{AudioOrigin, Located};
use crate::{
    Actor, AudioFile, Error, HostDeviceId, Result, Store,
    catalog::push_source_choice,
    library_storage::{ManagedFileLocation, ManagedFiles, managed_mime},
};
use futures_util::future::BoxFuture;
use sqlx::QueryBuilder;
use uuid::Uuid;

pub(crate) struct ManagedOrigin {
    pub store: Store,
    pub host: HostDeviceId,
    pub files: ManagedFiles,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ManagedLocation {
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

impl ManagedOrigin {
    async fn managed_location(
        &self,
        actor: &Actor,
        source: &str,
    ) -> Result<Option<ManagedLocation>> {
        let mut query = QueryBuilder::new(
            "SELECT file.id,file.attempt_id,file.generation,file.checksum,file.byte_length,file.mime,
                intent.id AS intent,intent.generation AS intent_generation,intent.access_grant_id AS access_grant
             FROM sources s JOIN recordings recording ON recording.id = s.recording_id
             JOIN download_files file ON file.source_id = s.id AND file.recording_id = recording.id
             JOIN download_intents intent ON intent.file_id = file.id AND intent.account_id = file.account_id
                AND intent.host_device = file.host_device AND intent.recording_id = file.recording_id
                AND intent.library_id = recording.library_id AND intent.state = 'ready'
             JOIN retained_download_intents retained ON retained.id=intent.id
             JOIN library_access_grants access ON access.library_id = intent.library_id
                AND access.user_id = intent.account_id AND access.id = intent.access_grant_id
             WHERE s.id = ",
        );
        query
            .push_bind(source.to_owned())
            .push(" AND s.provider='download' AND s.ready=1 AND s.device_id=")
            .push_bind(self.host.as_str().to_owned())
            .push(" AND file.host_device=")
            .push_bind(self.host.as_str().to_owned())
            .push(" AND file.account_id=")
            .push_bind(actor.id().to_owned())
            .push(" AND file.state='ready' AND ");
        push_source_choice(&mut query, actor.id());
        query.push(" ORDER BY intent.id LIMIT 1");
        let row: Option<Selected> = query
            .build_query_as()
            .fetch_optional(&self.store.pool)
            .await?;
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

impl AudioOrigin for ManagedOrigin {
    fn kind(&self) -> &'static str {
        "managed"
    }

    fn local(&self) -> bool {
        true
    }

    fn locate<'a>(
        &'a self,
        actor: &'a Actor,
        source: &'a str,
    ) -> BoxFuture<'a, Result<Option<Located>>> {
        Box::pin(async move {
            Ok(self
                .managed_location(actor, source)
                .await?
                .map(Located::new))
        })
    }

    fn still_valid<'a>(
        &'a self,
        actor: &'a Actor,
        source: &'a str,
        located: &'a Located,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            Ok(self.managed_location(actor, source).await?.as_ref()
                == Some(located.pinned::<ManagedLocation>()?))
        })
    }

    fn open_file<'a>(
        &'a self,
        actor: &'a Actor,
        source: &'a str,
    ) -> BoxFuture<'a, Result<Option<AudioFile>>> {
        Box::pin(async move {
            let Some(location) = self.managed_location(actor, source).await? else {
                return Ok(None);
            };
            let file = match self.files.open_file(&location.file).await {
                Ok(file) => file,
                Err(Error::NotFound) => {
                    crate::downloads::retire_unusable_copy(&self.store, &location.file).await?;
                    return Err(Error::NotFound);
                }
                Err(error) => return Err(error),
            };
            if self.managed_location(actor, source).await?.as_ref() != Some(&location) {
                return Err(Error::NotFound);
            }
            actor.check_session(&self.store).await?;
            tracing::debug!(event = "download_audio_opened");
            Ok(Some(file))
        })
    }
}
