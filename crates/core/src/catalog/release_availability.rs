use super::{
    Catalog, SourceAccess, push_source_access,
    releases::{ENTRIES, HAS_ENTRIES},
};
use crate::{Actor, Result};
use sqlx::{QueryBuilder, SqliteConnection};

#[derive(sqlx::FromRow)]
pub(super) struct ReleaseCounts {
    pub release_id: String,
    pub track_count: i64,
    pub playable_count: i64,
    pub contains_wanted: bool,
}

impl Catalog {
    /// Track and playable counts for the account's own albums. `wanted` holds
    /// `[release, recording]` pairs; an album reports whether it still lists that recording.
    pub(super) async fn release_counts_on(
        &self,
        connection: &mut SqliteConnection,
        actor: &Actor,
        wanted: &str,
    ) -> Result<Vec<ReleaseCounts>> {
        let mut query = QueryBuilder::new(
            "WITH wanted AS (SELECT json_extract(value, '$[0]') AS release_id, json_extract(value, '$[1]') AS recording_id
            FROM json_each(",
        );
        query.push_bind(wanted.to_owned());
        query
            .push("))\n, selected AS (SELECT a.* FROM account_releases a WHERE a.user_id = ")
            .push_bind(actor.id().to_owned())
            .push(" AND EXISTS (SELECT 1 FROM library_members access WHERE access.library_id = a.library_id AND access.user_id = ")
            .push_bind(actor.id().to_owned())
            .push(") AND a.id IN (SELECT release_id FROM wanted)")
            .push(HAS_ENTRIES)
            .push(ENTRIES)
            .push(
                " SELECT m.release_id, count(*) AS track_count, sum(EXISTS (SELECT 1 FROM sources s INDEXED BY sources_metadata_owner
                WHERE s.recording_id = m.recording_id AND ",
            );
        push_source_access(&mut query, &self.host, actor.id(), SourceAccess::Playable);
        query.push(
            ")) AS playable_count, max(m.recording_id IN (SELECT w.recording_id FROM wanted w WHERE w.release_id = m.release_id)) AS contains_wanted
            FROM entries m GROUP BY m.release_id",
        );
        Ok(query.build_query_as().fetch_all(connection).await?)
    }
}
