use super::{Browse, Catalog, Page, Release};
use crate::{Actor, Error, Result};
use serde::Serialize;
use sqlx::QueryBuilder;
use utoipa::ToSchema;

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct Artist {
    pub id: String,
    pub library_id: String,
    pub name: String,
    pub release_count: i64,
    pub artwork_recording_id: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct ArtistDetail {
    pub artist: Artist,
    pub releases: Page<Release>,
}

const SUMMARY: &str =
    "SELECT c.id, c.library_id, c.name, count(DISTINCT ar.release_id) AS release_count,
    min(s.recording_id) AS artwork_recording_id
    FROM artists c JOIN artist_releases ar ON ar.artist_id = c.id
    JOIN source_metadata m ON m.release_id = ar.release_id JOIN sources s ON s.id = m.source_id";

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn artists(&self, actor: &Actor, browse: Browse) -> Result<Page<Artist>> {
        let (offset, limit) = browse.bounds();
        let mut count = QueryBuilder::new("SELECT count(*) FROM artists c");
        browse.filter(&mut count, actor, "c", "c.name")?;
        count.push(" AND EXISTS (SELECT 1 FROM artist_releases ar WHERE ar.artist_id = c.id)");
        let total = count
            .build_query_scalar()
            .fetch_one(&self.store.pool)
            .await?;
        let mut query = QueryBuilder::new(SUMMARY);
        browse.filter(&mut query, actor, "c", "c.name")?;
        query
            .push(" GROUP BY c.id ORDER BY c.name COLLATE NOCASE, c.id LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);
        let items = query.build_query_as().fetch_all(&self.store.pool).await?;
        tracing::debug!(total, offset, limit, "Artist page loaded");
        Ok(Page {
            items,
            total,
            offset,
            limit,
        })
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), artist_id))]
    pub async fn artist(&self, actor: &Actor, id: &str, browse: Browse) -> Result<ArtistDetail> {
        let mut query = QueryBuilder::new(SUMMARY);
        Browse::default().filter(&mut query, actor, "c", "c.name")?;
        query
            .push(" AND c.id = ")
            .push_bind(id.to_owned())
            .push(" GROUP BY c.id");
        let artist: Artist = query
            .build_query_as()
            .fetch_optional(&self.store.pool)
            .await?
            .ok_or(Error::NotFound)?;
        tracing::Span::current().record("artist_id", &artist.id);
        let releases = self
            .releases(
                actor,
                Browse {
                    artist: Some(artist.id.clone()),
                    ..browse
                },
            )
            .await?;
        tracing::debug!(count = releases.items.len(), "Artist detail loaded");
        Ok(ArtistDetail { artist, releases })
    }
}
