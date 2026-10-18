use super::{Catalog, SourceAccess, push_source_access};
use crate::{Actor, Result};
use sqlx::QueryBuilder;
mod assets;
mod availability;
mod budget;
mod jobs;
mod lease;
mod playlist;
mod reader;
mod release;
mod target;
#[cfg(test)]
#[path = "artwork/tests/work.rs"]
mod work;
mod worker;
pub use availability::ArtworkAvailability;
pub use jobs::{ArtworkFetch, ArtworkFetchStatus};
pub use release::ReleaseArtworkFetch;

use reader::read_artwork;

pub struct Artwork {
    pub content_type: &'static str,
    pub bytes: Vec<u8>,
}

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), recording_id))]
    pub async fn artwork(&self, actor: &Actor, recording: &str) -> Result<Option<Artwork>> {
        actor.check_session(&self.store).await?;
        let sources = self.sources(actor, recording).await?;
        tracing::Span::current().record("recording_id", recording);
        let sources: Vec<String> = sources
            .into_iter()
            .filter(|source| source.ready && source.provider == "local")
            .take(4)
            .map(|source| source.id)
            .collect();
        let local = self.read_source_artwork(actor, &sources).await?;
        actor.check_session(&self.store).await?;
        if local.is_some() {
            return Ok(local);
        }
        self.retained_artwork(actor, recording).await
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), release_id))]
    pub async fn release_artwork(&self, actor: &Actor, release: &str) -> Result<Option<Artwork>> {
        actor.check_session(&self.store).await?;
        release::access_on(&mut *self.store.pool.acquire().await?, actor, release).await?;
        let mut query = QueryBuilder::new(
            "SELECT s.id FROM releases a
            JOIN library_members access ON access.library_id = a.library_id
            JOIN source_metadata m ON m.release_id = a.id JOIN sources s ON s.id = m.source_id
            JOIN recordings r ON r.id = s.recording_id AND r.library_id = a.library_id
            WHERE a.id = ",
        );
        query
            .push_bind(release.to_owned())
            .push(" AND access.user_id = ")
            .push_bind(actor.id().to_owned())
            .push(" AND s.provider = 'local' AND ");
        push_source_access(&mut query, &self.host, actor.id(), SourceAccess::LocalFile);
        query.push(" ORDER BY s.id LIMIT 4");
        let sources: Vec<String> = query
            .build_query_scalar()
            .fetch_all(&self.store.pool)
            .await?;
        if !sources.is_empty() {
            tracing::Span::current().record("release_id", release);
        }
        let local = self.read_source_artwork(actor, &sources).await?;
        actor.check_session(&self.store).await?;
        if local.is_some() {
            return Ok(local);
        }
        self.retained_release_artwork(actor, release).await
    }

    async fn read_source_artwork(
        &self,
        actor: &Actor,
        sources: &[String],
    ) -> Result<Option<Artwork>> {
        for source in sources {
            let permit = self
                .artwork_slots
                .clone()
                .acquire_owned()
                .await
                .map_err(|_| crate::Error::Internal("Artwork reader stopped".into()))?;
            let audio = match self.local_audio(actor, source).await {
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
