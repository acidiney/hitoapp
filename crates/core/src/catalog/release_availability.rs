use super::{Catalog, SourceAccess, push_source_access, releases::selected_among};
use crate::{Actor, Result};
use serde::Serialize;
use sqlx::SqliteConnection;

#[derive(sqlx::FromRow)]
pub(crate) struct ReleaseCounts {
    pub release_id: String,
    pub track_count: i64,
    pub playable_count: i64,
    pub contains_wanted: bool,
}

#[derive(Serialize)]
pub(crate) struct WantedRecording<'a> {
    pub release_id: &'a str,
    pub recording_id: &'a str,
}

impl Catalog {
    pub(crate) async fn release_counts_on(
        &self,
        connection: &mut SqliteConnection,
        actor: &Actor,
        wanted: &[WantedRecording<'_>],
    ) -> Result<Vec<ReleaseCounts>> {
        let mut query = selected_among(actor, wanted)?;
        query.push(
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
