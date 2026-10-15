use super::{Browse, Catalog, Page, Release};
use crate::{Actor, Error, Result};
use serde::Serialize;
use sqlx::{QueryBuilder, Sqlite};
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

const SUMMARY: &str = "), associations AS (
    SELECT c.id AS artist_id, a.id AS release_id FROM selected c
    CROSS JOIN releases a ON a.library_id = c.library_id AND a.artist = c.name COLLATE BINARY
    WHERE EXISTS (SELECT 1 FROM source_metadata m WHERE m.release_id = a.id)
    UNION ALL
    SELECT c.id, a.id FROM selected c
    CROSS JOIN source_artist_credits credit ON credit.artist_id = c.id
    CROSS JOIN source_metadata m ON m.source_id = credit.source_id
    CROSS JOIN releases a ON a.id = m.release_id AND a.library_id = c.library_id
), counts AS (
    SELECT c.id, c.library_id, c.name, count(DISTINCT ar.release_id) AS release_count, min(ar.release_id) AS cover_release
    FROM selected c JOIN associations ar ON ar.artist_id = c.id GROUP BY c.id
) SELECT c.id, c.library_id, c.name, c.release_count,
    (SELECT s.recording_id FROM source_metadata m JOIN sources s ON s.id = m.source_id
     WHERE m.release_id = c.cover_release ORDER BY m.source_id LIMIT 1) AS artwork_recording_id
    FROM counts c ORDER BY c.name COLLATE NOCASE, c.id";

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn artists(&self, actor: &Actor, browse: Browse) -> Result<Page<Artist>> {
        let (offset, limit) = browse.bounds();
        let mut count = QueryBuilder::new("SELECT count(*) FROM artists c");
        browse.filter(&mut count, actor, "c", "artists_fts")?;
        count.push(" AND EXISTS (SELECT 1 FROM artist_releases ar WHERE ar.artist_id = c.id)");
        let total = count
            .build_query_scalar()
            .fetch_one(&self.store.pool)
            .await?;
        let mut query = selected(actor, &browse, None)?;
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
        let mut query = selected(actor, &Browse::default(), Some(id))?;
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

fn selected(
    actor: &Actor,
    browse: &Browse,
    id: Option<&str>,
) -> Result<QueryBuilder<'static, Sqlite>> {
    let mut query = QueryBuilder::new("WITH selected AS (SELECT c.* FROM artists c");
    browse.filter(&mut query, actor, "c", "artists_fts")?;
    query.push(" AND EXISTS (SELECT 1 FROM artist_releases ar WHERE ar.artist_id = c.id)");
    if let Some(id) = id {
        query.push(" AND c.id = ").push_bind(id.to_owned());
    } else {
        let (offset, limit) = browse.bounds();
        query
            .push(" ORDER BY c.name COLLATE NOCASE, c.id LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);
    }
    query.push(SUMMARY);
    Ok(query)
}
