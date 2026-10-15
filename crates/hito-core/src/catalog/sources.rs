use super::{Catalog, Source, SourceAccess, push_source_access};
use crate::{Actor, Error, Result, identity::recording_access};
use sqlx::QueryBuilder;
use std::path::PathBuf;

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), host_device = self.host.as_str()))]
    pub async fn sources(&self, actor: &Actor, recording: &str) -> Result<Vec<Source>> {
        let mut connection = self.store.pool.acquire().await?;
        recording_access(&mut connection, actor, recording).await?;
        let mut query =
            QueryBuilder::new("SELECT s.id, s.recording_id, s.provider, s.device_id, (");
        push_source_access(&mut query, &self.host, SourceAccess::Playable);
        query
            .push(") AS ready FROM sources s WHERE s.recording_id = ")
            .push_bind(recording.to_owned())
            .push(" ORDER BY CASE s.provider WHEN 'local' THEN 0 ELSE 1 END, ready DESC, s.id");
        Ok(query.build_query_as().fetch_all(&mut *connection).await?)
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id(), host_device = self.host.as_str()))]
    pub async fn audio_file(&self, actor: &Actor, source: &str) -> Result<PathBuf> {
        let mut query = QueryBuilder::new(
            "SELECT f.path, f.scan_root
             FROM local_files f
             JOIN sources s ON s.id = f.source_id
             JOIN recordings r ON r.id = s.recording_id
             JOIN library_members m ON m.library_id = r.library_id
             WHERE s.id = ",
        );
        query
            .push_bind(source.to_owned())
            .push(" AND m.user_id = ")
            .push_bind(actor.id().to_owned())
            .push(" AND ");
        push_source_access(&mut query, &self.host, SourceAccess::LocalFile);
        let row: Option<(String, String)> = query
            .build_query_as()
            .fetch_optional(&self.store.pool)
            .await?;
        let (path, root) = row.ok_or(Error::NotFound)?;
        let path = tokio::fs::canonicalize(path)
            .await
            .map_err(|_| Error::NotFound)?;
        if !path.starts_with(root) {
            return Err(Error::NotFound);
        }
        Ok(path)
    }
}
