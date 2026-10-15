use super::{Catalog, SourceAccess, push_source_access};
use crate::{
    Actor, DirectoryRef, Error, Result,
    library_storage::{OpenedMedia, local_path},
};
use sqlx::QueryBuilder;

pub struct AudioFile {
    pub file: std::fs::File,
    pub length: u64,
    pub content_type: &'static str,
}

type Location = (String, String, String, i64);

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), source_id))]
    pub async fn audio_file(&self, actor: &Actor, source: &str) -> Result<AudioFile> {
        let media = self.local_audio(actor, source).await?;
        tracing::Span::current().record("source_id", source);
        let length = media.file.metadata().map_err(|_| Error::NotFound)?.len();
        let content_type = match media
            .path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "wav" => "audio/wav",
            "flac" => "audio/flac",
            "mp3" => "audio/mpeg",
            "ogg" | "opus" => "audio/ogg",
            "m4a" | "mp4" => "audio/mp4",
            "aac" => "audio/aac",
            "aif" | "aiff" => "audio/aiff",
            "wma" => "audio/x-ms-wma",
            _ => "application/octet-stream",
        };
        Ok(AudioFile {
            file: media.file,
            length,
            content_type,
        })
    }

    pub(super) async fn local_audio(&self, actor: &Actor, source: &str) -> Result<OpenedMedia> {
        let location = self.media_location(actor, source).await?;
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
        if self.media_location(actor, source).await? != location {
            return Err(Error::NotFound);
        }
        Ok(media)
    }

    async fn media_location(&self, actor: &Actor, source: &str) -> Result<Location> {
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
        push_source_access(&mut query, &self.host, SourceAccess::LocalFile);
        query
            .build_query_as()
            .fetch_optional(&self.store.pool)
            .await?
            .ok_or(Error::NotFound)
    }
}
