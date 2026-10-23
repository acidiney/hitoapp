use super::origin::{AudioOrigin, Located};
use crate::{
    Actor, AudioFile, Error, Result,
    catalog::local_files::{IndexedLocation, LocalFiles},
};
use futures_util::future::BoxFuture;

pub(crate) struct LocalOrigin {
    pub files: LocalFiles,
}

impl LocalOrigin {
    async fn open_indexed(
        &self,
        actor: &Actor,
        source: &str,
        location: &IndexedLocation,
    ) -> Result<AudioFile> {
        let media = self.files.open(actor, source, location).await?;
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
}

impl AudioOrigin for LocalOrigin {
    fn kind(&self) -> &'static str {
        "local"
    }

    fn local(&self) -> bool {
        true
    }

    fn locate<'a>(
        &'a self,
        actor: &'a Actor,
        source: &'a str,
    ) -> BoxFuture<'a, Result<Option<Located>>> {
        Box::pin(async move { Ok(self.files.location(actor, source).await?.map(Located::new)) })
    }

    fn still_valid<'a>(
        &'a self,
        actor: &'a Actor,
        source: &'a str,
        located: &'a Located,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            Ok(self.files.location(actor, source).await?.as_ref()
                == Some(located.pinned::<IndexedLocation>()?))
        })
    }

    fn open_file<'a>(
        &'a self,
        actor: &'a Actor,
        source: &'a str,
    ) -> BoxFuture<'a, Result<Option<AudioFile>>> {
        Box::pin(async move {
            let Some(location) = self.files.location(actor, source).await? else {
                return Ok(None);
            };
            self.open_indexed(actor, source, &location).await.map(Some)
        })
    }
}
