use super::Catalog;
use crate::{Actor, Result};
mod budget;
mod reader;

use reader::read_artwork;

pub struct Artwork {
    pub content_type: &'static str,
    pub bytes: Vec<u8>,
}

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), recording_id = recording))]
    pub async fn artwork(&self, actor: &Actor, recording: &str) -> Result<Option<Artwork>> {
        let sources = self.sources(actor, recording).await?;
        for source in sources
            .iter()
            .filter(|source| source.ready && source.provider == "local")
            .take(4)
        {
            let permit = self
                .artwork_slots
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| crate::Error::Internal("Artwork reader stopped".into()))?;
            let audio = match self.local_audio(actor, &source.id).await {
                Ok(audio) => audio,
                Err(crate::Error::NotFound) => continue,
                Err(error) => return Err(error),
            };
            let span = tracing::Span::current();
            let artwork = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                span.in_scope(|| read_artwork(audio))
            })
            .await
            .map_err(|_| crate::Error::Internal("Artwork reader stopped".into()))?;
            if artwork.is_some() {
                tracing::debug!(event = "artwork_resolved", "Library artwork resolved");
                return Ok(artwork);
            }
        }
        Ok(None)
    }
}
