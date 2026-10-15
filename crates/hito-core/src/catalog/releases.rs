use super::{Browse, Catalog, Page, Recording, push_available};
use crate::{Actor, Error, Result, metadata};
use serde::Serialize;
use sqlx::{QueryBuilder, Sqlite};
use utoipa::ToSchema;

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct Release {
    pub id: String,
    pub library_id: String,
    pub title: String,
    pub artist: String,
    pub artist_id: String,
    pub track_count: i64,
    pub duration_ms: i64,
    pub artwork_recording_id: Option<String>,
}

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct ReleaseTrack {
    #[serde(flatten)]
    #[sqlx(flatten)]
    pub recording: Recording,
    pub entry_id: String,
    pub disc_number: Option<i64>,
    pub track_number: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub struct ReleaseDetail {
    pub release: Release,
    pub tracks: Page<ReleaseTrack>,
}

const ENTRIES: &str = "), entries AS (
    SELECT m.release_id, s.recording_id, min(m.source_id) AS source_id
    FROM selected a CROSS JOIN source_metadata m ON m.release_id = a.id JOIN sources s ON s.id = m.source_id
    GROUP BY m.release_id, s.recording_id, m.disc_number, m.track_number
) ";
const SUMMARY: &str = "SELECT a.id, a.library_id, a.title, a.artist, a.artist_id,
    count(*) AS track_count, sum(m.duration_ms) AS duration_ms,
    min(e.recording_id) AS artwork_recording_id
    FROM selected a JOIN entries e ON e.release_id = a.id
    JOIN source_metadata m ON m.source_id = e.source_id";

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn releases(&self, actor: &Actor, browse: Browse) -> Result<Page<Release>> {
        let (offset, limit) = browse.bounds();
        let mut count = QueryBuilder::new("SELECT count(*) FROM account_releases a");
        browse.filter(
            &mut count,
            actor,
            "a",
            "account_releases_fts",
            "{title artist}",
        )?;
        filter_artist(&mut count, &browse);
        count.push(" AND EXISTS (SELECT 1 FROM source_metadata m WHERE m.release_id = a.id)");
        let total = count
            .build_query_scalar()
            .fetch_one(&self.store.pool)
            .await?;
        let mut query = selected(actor, &browse, None)?;
        query.push(SUMMARY);
        query.push(" GROUP BY a.id ORDER BY a.title COLLATE NOCASE, a.id");
        let items = query.build_query_as().fetch_all(&self.store.pool).await?;
        tracing::debug!(total, offset, limit, "Album page loaded");
        Ok(Page {
            items,
            total,
            offset,
            limit,
        })
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), release_id))]
    pub async fn release(&self, actor: &Actor, id: &str, browse: Browse) -> Result<ReleaseDetail> {
        let (offset, limit) = browse.bounds();
        let mut summary = selected(actor, &Browse::default(), Some(id))?;
        summary.push(SUMMARY);
        summary.push(" GROUP BY a.id");
        let release: Release = summary
            .build_query_as()
            .fetch_optional(&self.store.pool)
            .await?
            .ok_or(Error::NotFound)?;
        tracing::Span::current().record("release_id", &release.id);
        let mut query = selected(actor, &Browse::default(), Some(&release.id))?;
        query
            .push("SELECT r.id, r.library_id, a.id AS release_id, ")
            .push(metadata::APPEARANCE_TITLE)
            .push(" AS title, m.title AS inherited_title, ")
            .push(metadata::APPEARANCE_ARTIST)
            .push(" AS artist, m.artist AS inherited_artist, a.title AS album, m.duration_ms, ");
        push_available(&mut query, &self.host);
        query
            .push(
                " AS available, COALESCE(l.liked, 0) AS liked, m.source_id AS entry_id, m.disc_number, m.track_number FROM entries e
            JOIN selected a ON a.id = e.release_id JOIN recordings r ON r.id = e.recording_id
            JOIN source_metadata m ON m.source_id = e.source_id
            LEFT JOIN likes l ON l.recording_id = r.id AND l.user_id = ",
            )
            .push_bind(actor.id().to_owned());
        metadata::join_recording_fields(&mut query, actor);
        query
            .push(" ORDER BY COALESCE(m.disc_number, 1), m.track_number IS NULL, m.track_number, title COLLATE NOCASE, r.id, m.source_id LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);
        let items = query.build_query_as().fetch_all(&self.store.pool).await?;
        let total = release.track_count;
        tracing::debug!(total, offset, limit, "Album detail loaded");
        Ok(ReleaseDetail {
            release,
            tracks: Page {
                items,
                total,
                offset,
                limit,
            },
        })
    }
}

fn filter_artist(query: &mut QueryBuilder<'_, Sqlite>, browse: &Browse) {
    if let Some(artist) = &browse.artist {
        query.push(" AND EXISTS (SELECT 1 FROM account_artist_releases ar WHERE ar.user_id = a.user_id AND ar.release_id = a.id AND ar.artist_id = ").push_bind(artist.clone()).push(")");
    }
}

fn selected(
    actor: &Actor,
    browse: &Browse,
    id: Option<&str>,
) -> Result<QueryBuilder<'static, Sqlite>> {
    let mut query = QueryBuilder::new("WITH selected AS (SELECT a.* FROM account_releases a");
    browse.filter(
        &mut query,
        actor,
        "a",
        "account_releases_fts",
        "{title artist}",
    )?;
    filter_artist(&mut query, browse);
    query.push(" AND EXISTS (SELECT 1 FROM source_metadata m WHERE m.release_id = a.id)");
    if let Some(id) = id {
        query.push(" AND a.id = ").push_bind(id.to_owned());
    } else {
        let (offset, limit) = browse.bounds();
        query
            .push(" ORDER BY a.title COLLATE NOCASE, a.id LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);
    }
    query.push(ENTRIES);
    Ok(query)
}
