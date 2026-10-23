use super::{SourceAccess, push_source_access};
use crate::{
    Actor, DirectoryRef, Error, HostDeviceId, Result, Store,
    library_storage::{Directories, OpenedMedia, local_path},
};
use sqlx::QueryBuilder;

pub(crate) type IndexedLocation = (String, String, String, i64);

#[derive(Clone)]
pub(crate) struct LocalFiles {
    pub store: Store,
    pub host: HostDeviceId,
    pub directories: Directories,
}

impl LocalFiles {
    pub async fn location(&self, actor: &Actor, source: &str) -> Result<Option<IndexedLocation>> {
        let mut query = QueryBuilder::new(
            "SELECT f.relative_path, folder.directory_id, folder.relative_path, folder.generation
             FROM local_files f JOIN music_folders folder ON folder.id = f.folder_id
             JOIN sources s ON s.id = f.source_id JOIN recordings r ON r.id = s.recording_id
             JOIN library_members m ON m.library_id = r.library_id WHERE s.id = ",
        );
        query
            .push_bind(source.to_owned())
            .push(" AND m.user_id = ")
            .push_bind(actor.id().to_owned())
            .push(" AND ");
        push_source_access(&mut query, &self.host, actor.id(), SourceAccess::LocalFile);
        Ok(query
            .build_query_as()
            .fetch_optional(&self.store.pool)
            .await?)
    }

    pub async fn open(
        &self,
        actor: &Actor,
        source: &str,
        location: &IndexedLocation,
    ) -> Result<OpenedMedia> {
        let selection = DirectoryRef {
            directory_id: location.1.clone(),
            components: location
                .2
                .split('/')
                .filter(|part| !part.is_empty())
                .map(str::to_owned)
                .collect(),
        };
        let media = self
            .directories
            .open_media(&selection, local_path::decode(&location.0)?)
            .await?;
        if self.location(actor, source).await?.as_ref() != Some(location) {
            return Err(Error::NotFound);
        }
        Ok(media)
    }

    pub async fn audio(&self, actor: &Actor, source: &str) -> Result<OpenedMedia> {
        let location = self.location(actor, source).await?.ok_or(Error::NotFound)?;
        self.open(actor, source, &location).await
    }
}
