use super::{Browse, Catalog, Page, Recording, Release, Search};
use crate::artist_catalog::identity::{ChosenRecordings, CreditAlias};
use crate::{Actor, Error, Result};
use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, sqlx::FromRow, ToSchema)]
pub struct Artist {
    pub id: String,
    pub library_id: String,
    pub name: String,
    pub release_count: i64,
    pub recording_count: i64,
    pub artwork_recording_id: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub provider_photo: bool,
}

#[derive(Serialize, ToSchema)]
pub struct ArtistDetail {
    pub artist: Artist,
    pub releases: Page<Release>,
    pub recordings: Page<Recording>,
}

const SUMMARY_HEAD: &str = "), associations AS (
    SELECT c.user_id, c.id AS artist_id, appearance.release_id FROM selected c
    CROSS JOIN account_artist_releases appearance ON appearance.user_id = c.user_id AND appearance.artist_id = c.id
), counts AS (
    SELECT c.user_id, c.id, c.library_id, c.name, count(DISTINCT ar.release_id) AS release_count, min(ar.release_id) AS cover_release
    FROM selected c LEFT JOIN associations ar ON ar.user_id = c.user_id AND ar.artist_id = c.id GROUP BY c.id
) SELECT c.id, c.library_id, c.name, c.release_count,
    (SELECT count(*) FROM account_recording_credits rc WHERE rc.user_id = c.user_id AND rc.artist_id = c.id) AS recording_count,
    COALESCE((SELECT s.recording_id FROM source_metadata m JOIN sources s ON s.id = m.source_id
     WHERE m.release_id = c.cover_release ORDER BY m.source_id LIMIT 1),
    (SELECT appearance.recording_id FROM provider_release_appearances appearance
     WHERE appearance.release_id = c.cover_release ORDER BY appearance.recording_id LIMIT 1),
    (SELECT rc.recording_id FROM account_recording_credits rc WHERE rc.user_id = c.user_id AND rc.artist_id = c.id ORDER BY rc.recording_id LIMIT 1)) AS artwork_recording_id,
    ";

const SUMMARY_TAIL: &str = " AS provider_photo
    FROM counts c ORDER BY c.name COLLATE NOCASE, c.id";

const ACCOUNT_ARTIST: CreditAlias = CreditAlias::new("c");

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn artists(&self, actor: &Actor, browse: Browse) -> Result<Page<Artist>> {
        let (offset, limit) = browse.bounds();
        let mut count = ChosenRecordings::new(actor);
        count.push(" SELECT count(*) FROM account_artists c");
        browse.filter(&mut count, actor, "c", "account_artists_fts", "name")?;
        count.push(" AND (EXISTS (SELECT 1 FROM account_artist_releases ar WHERE ar.user_id = c.user_id AND ar.artist_id = c.id) OR EXISTS (SELECT 1 FROM account_recording_credits rc WHERE rc.user_id = c.user_id AND rc.artist_id = c.id))");
        count.exclude_represented(ACCOUNT_ARTIST);
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
        let recordings = self
            .recordings(
                actor,
                Search {
                    artist: Some(artist.id.clone()),
                    library: browse.library.clone(),
                    offset: browse.recording_offset,
                    limit: Some(browse.bounds().1),
                    ..Search::default()
                },
            )
            .await?;
        let releases = self
            .releases(
                actor,
                Browse {
                    artist: Some(artist.id.clone()),
                    ..browse
                },
            )
            .await?;
        tracing::debug!(
            releases = releases.items.len(),
            recordings = recordings.items.len(),
            "Artist detail loaded"
        );
        Ok(ArtistDetail {
            artist,
            releases,
            recordings,
        })
    }
}

fn selected(actor: &Actor, browse: &Browse, id: Option<&str>) -> Result<ChosenRecordings> {
    let mut query = ChosenRecordings::new(actor);
    query.push(", selected AS (SELECT c.* FROM account_artists c");
    browse.filter(&mut query, actor, "c", "account_artists_fts", "name")?;
    query.push(" AND (EXISTS (SELECT 1 FROM account_artist_releases ar WHERE ar.user_id = c.user_id AND ar.artist_id = c.id) OR EXISTS (SELECT 1 FROM account_recording_credits rc WHERE rc.user_id = c.user_id AND rc.artist_id = c.id))");
    if let Some(id) = id {
        query.push(" AND c.id = ").push_bind(id.to_owned());
    } else {
        query.exclude_represented(ACCOUNT_ARTIST);
        let (offset, limit) = browse.bounds();
        query
            .push(" ORDER BY c.name COLLATE NOCASE, c.id LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);
    }
    query.push(SUMMARY_HEAD);
    query.push(super::artwork::credited_artist_photo_exists("c"));
    query.push(SUMMARY_TAIL);
    Ok(query)
}
