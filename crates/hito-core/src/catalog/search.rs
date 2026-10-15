use super::{Catalog, Page, Recording, Search, push_available};
use crate::{Actor, Error, Library, Result};
use sqlx::{QueryBuilder, Sqlite};

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn libraries(&self, actor: &Actor) -> Result<Vec<Library>> {
        Ok(sqlx::query_as(
            "SELECT l.id, l.name
             FROM libraries l JOIN library_members m ON l.id = m.library_id
             WHERE m.user_id = ? ORDER BY l.name",
        )
        .bind(actor.id())
        .fetch_all(&self.store.pool)
        .await?)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), host_device = self.host.as_str()))]
    pub async fn recordings(&self, actor: &Actor, search: Search) -> Result<Page<Recording>> {
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
            push_available(&mut query, &self.host);
            query.push(" AS available, COALESCE(l.liked, 0) AS liked");
            filters(&mut query, actor, &search, &terms, personal, true);
        }
        query
            .push(" ORDER BY title COLLATE NOCASE, r.id LIMIT ")
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

pub(super) fn search_terms(query: &str) -> Result<String> {
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
    if search.liked == Some(true) {
        query.push(" AND l.liked = 1");
    }
    if let Some(library) = &search.library {
        query
            .push(" AND r.library_id = ")
            .push_bind(library.clone());
    }
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
