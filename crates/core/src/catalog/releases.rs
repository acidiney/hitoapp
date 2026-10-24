use super::{
    AlbumEditionChoice, Browse, Catalog, Page, Recording, SourceAccess, push_source_access,
};
use crate::{Actor, Error, Result, metadata};
use serde::Serialize;
use sqlx::{QueryBuilder, Sqlite};
use utoipa::ToSchema;

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct Release {
    pub id: String,
    pub library_id: String,
    pub title: String,
    #[schema(required = true)]
    pub artist: Option<String>,
    #[schema(required = true)]
    pub artist_id: Option<String>,
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

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct AlbumCatalogAnchor {
    pub recording_id: String,
    pub association_id: String,
}

#[derive(Serialize, ToSchema)]
pub struct ReleaseDetail {
    pub release: Release,
    pub tracks: Page<ReleaseTrack>,
    #[schema(required = true)]
    pub catalog_anchor: Option<AlbumCatalogAnchor>,
    #[schema(required = true)]
    pub saved_edition: Option<AlbumEditionChoice>,
}

pub(super) const MAX_COMPARED_TRACKS: i64 = 1000;
const MAX_ANCHOR_CANDIDATES: i64 = 16;

const HAS_ENTRIES: &str = " AND (EXISTS (SELECT 1 FROM source_metadata m WHERE m.release_id = a.id)
    OR EXISTS (SELECT 1 FROM provider_release_appearances appearance WHERE appearance.release_id = a.id))";
const ENTRIES: &str = "), source_entries AS (
    SELECT m.release_id, s.recording_id, min(m.source_id) AS entry_id
    FROM selected a CROSS JOIN source_metadata m ON m.release_id = a.id JOIN sources s ON s.id = m.source_id
    GROUP BY m.release_id, s.recording_id, m.disc_number, m.track_number
), entries AS (
    SELECT e.release_id, e.recording_id, e.entry_id, m.title, m.artist, m.duration_ms, m.disc_number, m.track_number
    FROM source_entries e JOIN source_metadata m ON m.source_id = e.entry_id
    UNION ALL
    SELECT appearance.release_id, appearance.recording_id, appearance.entry_id,
        appearance.title, appearance.artist, appearance.duration_ms, appearance.disc_number, appearance.track_number
    FROM selected a
    CROSS JOIN provider_release_appearances appearance ON appearance.release_id = a.id
    WHERE NOT EXISTS (
        SELECT 1 FROM sources s CROSS JOIN source_metadata m ON m.source_id = s.id
        WHERE s.recording_id = appearance.recording_id AND m.release_id = appearance.release_id
            AND m.disc_number IS appearance.disc_number AND m.track_number IS appearance.track_number
    )
) ";
const SUMMARY: &str = "SELECT a.id, a.library_id, a.title, a.artist, a.artist_id,
    count(*) AS track_count, sum(m.duration_ms) AS duration_ms,
    min(m.recording_id) AS artwork_recording_id
    FROM selected a JOIN entries m ON m.release_id = a.id";

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
        count.push(HAS_ENTRIES);
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
        self.release_page(actor, id, offset, limit, offset == 0)
            .await
    }

    pub(super) async fn release_for_comparison(
        &self,
        actor: &Actor,
        id: &str,
    ) -> Result<ReleaseDetail> {
        let detail = self
            .release_page(actor, id, 0, MAX_COMPARED_TRACKS, false)
            .await?;
        if detail.tracks.total > MAX_COMPARED_TRACKS {
            return Err(Error::Invalid(
                "Albums with more than 1,000 tracks cannot be compared with a catalog edition"
                    .into(),
            ));
        }
        Ok(detail)
    }

    async fn release_page(
        &self,
        actor: &Actor,
        id: &str,
        offset: i64,
        limit: i64,
        with_anchor: bool,
    ) -> Result<ReleaseDetail> {
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
        query.push("EXISTS (SELECT 1 FROM sources s INDEXED BY sources_metadata_owner WHERE s.recording_id = r.id AND ");
        push_source_access(&mut query, &self.host, actor.id(), SourceAccess::Playable);
        query.push(")");
        query
            .push(
                " AS available, COALESCE(l.liked, 0) AS liked, m.entry_id, m.disc_number, m.track_number FROM entries m
            JOIN selected a ON a.id = m.release_id JOIN recordings r ON r.id = m.recording_id
            LEFT JOIN likes l ON l.recording_id = r.id AND l.user_id = ",
            )
            .push_bind(actor.id().to_owned());
        metadata::join_recording_fields(&mut query, actor);
        query
            .push(" ORDER BY COALESCE(m.disc_number, 1), m.track_number IS NULL, m.track_number, title COLLATE NOCASE, r.id, m.entry_id LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);
        let items = query.build_query_as().fetch_all(&self.store.pool).await?;
        let total = release.track_count;
        let saved_edition = if with_anchor {
            Some(self.saved_album_edition(actor, &release.id).await?)
        } else {
            None
        };
        let catalog_anchor = if with_anchor {
            let mut candidates = selected(actor, &Browse::default(), Some(&release.id))?;
            candidates
                .push(
                    "SELECT assoc.recording_id, assoc.id AS association_id FROM entries m
                    JOIN recording_artist_associations assoc ON assoc.recording_id = m.recording_id AND assoc.user_id = ",
                )
                .push_bind(actor.id().to_owned())
                .push(" ORDER BY (assoc.artist_id IS ")
                .push_bind(release.artist_id.clone())
                .push(") DESC, COALESCE(m.disc_number, 1), m.track_number IS NULL, m.track_number, assoc.created_at, assoc.id LIMIT ")
                .push_bind(MAX_ANCHOR_CANDIDATES);
            let candidates = candidates
                .build_query_as()
                .fetch_all(&self.store.pool)
                .await?;
            self.first_readable_anchor(actor, candidates).await?
        } else {
            None
        };
        tracing::debug!(total, offset, limit, "Album detail loaded");
        Ok(ReleaseDetail {
            catalog_anchor,
            saved_edition,
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

pub(super) async fn release_contains(
    connection: &mut sqlx::SqliteConnection,
    actor: &Actor,
    release: &str,
    recording: Option<&str>,
) -> Result<bool> {
    let mut query = selected(actor, &Browse::default(), Some(release))?;
    match recording {
        Some(recording) => query
            .push("SELECT 1 FROM entries m WHERE m.recording_id = ")
            .push_bind(recording.to_owned()),
        None => query.push("SELECT 1 FROM selected"),
    };
    query.push(" LIMIT 1");
    Ok(query
        .build_query_scalar::<i64>()
        .fetch_optional(connection)
        .await?
        .is_some())
}

fn filter_artist(query: &mut QueryBuilder<'_, Sqlite>, browse: &Browse) {
    if let Some(artist) = &browse.artist {
        query.push(" AND EXISTS (SELECT 1 FROM account_artist_releases ar WHERE ar.user_id = a.user_id AND ar.release_id = a.id AND ar.artist_id = ").push_bind(artist.clone()).push(")");
    }
}

pub(super) async fn releases_by_ids_on(
    connection: &mut sqlx::SqliteConnection,
    actor: &Actor,
    ids: &[String],
) -> Result<Vec<Release>> {
    if ids.len() > 200 {
        return Err(Error::Invalid(
            "Select at most 200 releases per lookup".into(),
        ));
    }
    let requested = serde_json::to_string(ids)
        .map_err(|_| Error::Internal("Release selection could not be encoded".into()))?;
    let mut query = QueryBuilder::new("WITH selected AS (SELECT a.* FROM account_releases a");
    Browse::default().filter(
        &mut query,
        actor,
        "a",
        "account_releases_fts",
        "{title artist}",
    )?;
    query
        .push(" AND a.id IN (SELECT value FROM json_each(")
        .push_bind(requested)
        .push("))");
    query.push(HAS_ENTRIES).push(ENTRIES).push(SUMMARY);
    query.push(" GROUP BY a.id ORDER BY a.title COLLATE NOCASE,a.id");
    let releases: Vec<Release> = query.build_query_as().fetch_all(connection).await?;
    if releases.len() != ids.len() {
        return Err(Error::NotFound);
    }
    Ok(releases)
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
    query.push(HAS_ENTRIES);
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
