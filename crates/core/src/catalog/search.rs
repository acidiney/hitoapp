use super::{
    Catalog, CatalogSort, Page, Recording, Search, push_available, push_plays_from_services,
    push_primary_artist,
};
use crate::{Actor, Error, Library, Result, identity::recording_access};
use sqlx::{QueryBuilder, Sqlite};

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn recording_liked(&self, actor: &Actor, recording: &str) -> Result<bool> {
        let mut connection = self.store.pool.acquire().await?;
        actor.check_session_on(&mut connection).await?;
        recording_access(&mut connection, actor, recording).await?;
        Ok(sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM likes WHERE user_id = ? AND recording_id = ?)",
        )
        .bind(actor.id())
        .bind(recording)
        .fetch_one(&mut *connection)
        .await?)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn libraries(&self, actor: &Actor) -> Result<Vec<Library>> {
        Ok(sqlx::query_as(
            "SELECT l.id, l.name, l.owner_id IS NOT NULL AS personal
             FROM libraries l JOIN library_members m ON l.id = m.library_id
             WHERE m.user_id = ? ORDER BY personal, l.name",
        )
        .bind(actor.id())
        .fetch_all(&self.store.pool)
        .await?)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn local_file_count(&self, actor: &Actor) -> Result<i64> {
        Ok(sqlx::query_scalar(
            "SELECT count(*) FROM local_files f
             JOIN library_members m ON m.library_id = f.library_id
             WHERE m.user_id = ?",
        )
        .bind(actor.id())
        .fetch_one(&self.store.pool)
        .await?)
    }

    pub async fn liked_recordings(&self, actor: &Actor, search: Search) -> Result<Page<Recording>> {
        self.recordings(
            actor,
            Search {
                liked: Some(true),
                ..search
            },
        )
        .await
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), host_device = self.host.as_str()))]
    pub async fn recordings(&self, actor: &Actor, search: Search) -> Result<Page<Recording>> {
        let order = match search.sort.unwrap_or_default() {
            CatalogSort::Title => "title COLLATE NOCASE, r.id",
            CatalogSort::Recent => "added_rank DESC, r.id",
            CatalogSort::Artist => "artist COLLATE NOCASE, title COLLATE NOCASE, r.id",
            CatalogSort::Year => {
                return Err(Error::Invalid("Tracks cannot be sorted by year".into()));
            }
        };
        let offset = search.offset.unwrap_or(0).max(0);
        let limit = search.limit.unwrap_or(100).clamp(1, 200);
        let terms = search_terms(search.q.as_deref().unwrap_or_default())?;
        let mut count = QueryBuilder::new("SELECT sum(n) FROM (SELECT count(*) AS n");
        filters(&mut count, actor, &search, &terms, false, false);
        count.push(" UNION ALL SELECT count(*) AS n");
        filters(&mut count, actor, &search, &terms, true, false);
        count.push(")");
        let total = count
            .build_query_scalar()
            .fetch_one(&self.store.pool)
            .await?;

        let mut query = QueryBuilder::new("");
        for personal in [false, true] {
            if personal {
                query.push(" UNION ALL ");
            }
            query
                .push("SELECT r.id, r.library_id, r.release_id, ")
                .push(if personal { "p.title" } else { "r.title" })
                .push(" AS title, r.title AS inherited_title, ")
                .push(if personal { "p.artist" } else { "r.artist" })
                .push(
                    " AS artist, r.artist AS inherited_artist, a.title AS album, r.duration_ms, ",
                );
            push_available(&mut query, &self.host, actor.id());
            query.push(" AS available, ");
            push_primary_artist(&mut query, actor.id());
            query.push(" AS artist_id, ");
            push_plays_from_services(&mut query, actor.id());
            query.push(
                " AS plays_from_services, COALESCE(l.liked, 0) AS liked, r.rowid AS added_rank",
            );
            filters(&mut query, actor, &search, &terms, personal, true);
        }
        query
            .push(" ORDER BY ")
            .push(order)
            .push(" LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);
        let items = query.build_query_as().fetch_all(&self.store.pool).await?;
        tracing::debug!(total, offset, limit, "Catalog page loaded");
        Ok(Page {
            items,
            total,
            offset,
            limit,
        })
    }
}

pub(crate) fn search_terms(query: &str) -> Result<String> {
    if query.len() > 512 {
        return Err(Error::Invalid("Search must be at most 512 bytes".into()));
    }
    Ok(query
        .split_whitespace()
        .map(|term| format!("\"{}\"*", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" AND "))
}

fn filters(
    query: &mut QueryBuilder<'_, Sqlite>,
    actor: &Actor,
    search: &Search,
    terms: &str,
    personal: bool,
    ordered: bool,
) {
    if personal {
        query.push(
            " FROM personal_recording_metadata p CROSS JOIN recordings r ON r.id = p.recording_id",
        );
    } else {
        query.push(" FROM recordings r");
    }
    if ordered {
        query
            .push(" LEFT JOIN account_releases a ON a.id = r.release_id AND a.user_id = ")
            .push_bind(actor.id().to_owned());
    }
    if ordered || search.liked == Some(true) {
        query
            .push(" LEFT JOIN likes l ON l.recording_id = r.id AND l.user_id = ")
            .push_bind(actor.id().to_owned());
    }
    query
        .push(
            " WHERE EXISTS (
             SELECT 1 FROM library_members m
             WHERE m.library_id = r.library_id AND m.user_id = ",
        )
        .push_bind(actor.id().to_owned())
        .push(")");
    if personal {
        query
            .push(" AND p.user_id = ")
            .push_bind(actor.id().to_owned());
    } else {
        query
            .push(" AND NOT EXISTS (SELECT 1 FROM personal_recording_metadata p WHERE p.user_id = ")
            .push_bind(actor.id().to_owned())
            .push(" AND p.recording_id = r.id)");
    }
    if !terms.is_empty() {
        query.push(if personal {
            " AND p.id IN (SELECT rowid FROM personal_recordings_fts WHERE personal_recordings_fts MATCH "
        } else {
            " AND r.rowid IN (SELECT rowid FROM recordings_fts WHERE recordings_fts MATCH "
        }).push_bind(if personal { scoped_terms(actor, "{title artist}", terms) } else { terms.to_owned() }).push(")");
    }
    if terms.is_empty() {
        super::groups::push_cards_only(query, actor.id());
    }
    if search.liked == Some(true) {
        query.push(" AND l.liked = 1");
    }
    if let Some(artist) = &search.artist {
        push_credited_to(query, actor, artist, true);
    }
    if let Some(artist) = &search.featured_artist {
        push_credited_to(query, actor, artist, false);
    }
    if let Some(library) = &search.library {
        query
            .push(" AND r.library_id = ")
            .push_bind(library.clone());
    }
}

fn push_credited_to(query: &mut QueryBuilder<'_, Sqlite>, actor: &Actor, artist: &str, main: bool) {
    query.push(" AND EXISTS (SELECT 1 FROM (");
    if main {
        query
            .push("SELECT recording_id FROM account_recording_credits WHERE user_id = ")
            .push_bind(actor.id().to_owned())
            .push(" AND artist_id = ")
            .push_bind(artist.to_owned())
            .push(" UNION ALL ");
    }
    query
        .push("SELECT recording_id FROM account_recording_credit_members WHERE user_id = ")
        .push_bind(actor.id().to_owned())
        .push(" AND artist_id = ")
        .push_bind(artist.to_owned())
        .push(" AND role = ")
        .push(if main { "'main'" } else { "'featured'" })
        .push(") credit WHERE credit.recording_id = r.id OR credit.recording_id IN (SELECT grouped.recording_id FROM recording_group_members grouped WHERE grouped.user_id = ")
        .push_bind(actor.id().to_owned())
        .push(" AND grouped.canonical_id = r.id))");
}

pub(crate) fn scoped_terms(actor: &Actor, columns: &str, terms: &str) -> String {
    let scope: String = actor
        .id()
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect();
    format!("scope: \"{scope}\" AND {columns}: ({terms})")
}
