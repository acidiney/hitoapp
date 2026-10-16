use crate::{Error, Result, store::Store};
use std::time::Duration;

#[derive(Clone)]
pub(crate) struct Authority {
    pub store: Store,
    pub account: String,
    pub installation: String,
    pub service: String,
    pub credential: Option<Vec<u8>>,
    pub source: String,
    pub remote_source: String,
    pub generation: i64,
}
impl Authority {
    pub(in crate::plugins) async fn while_valid<T>(
        &self,
        future: impl std::future::Future<Output = Result<T>>,
    ) -> Result<T> {
        let mut pending = std::pin::pin!(future);
        loop {
            tokio::select! {
                result = &mut pending => return result,
                _ = tokio::time::sleep(Duration::from_millis(250)) => self.check().await?,
            }
        }
    }
    #[tracing::instrument(level = "trace", skip_all, err(Debug))]
    pub(crate) async fn check(&self) -> Result<()> {
        let allowed:bool = tokio::time::timeout(Duration::from_secs(1), sqlx::query_scalar("SELECT EXISTS(
            SELECT 1 FROM plugin_installations p JOIN plugin_services service ON service.id=p.service_id AND service.active=1
            JOIN provider_sources ps ON ps.installation_id=p.id JOIN sources s ON s.id=ps.source_id AND s.recording_id=ps.recording_id
            JOIN recordings r ON r.id=s.recording_id JOIN library_members m ON m.library_id=r.library_id AND m.user_id=p.user_id
            WHERE p.id=? AND p.user_id=? AND p.service_id=? AND p.active=1 AND p.credential IS ? AND p.audio_generation=?
            AND ps.source_id=? AND ps.provider_source_id=?
            AND EXISTS(SELECT 1 FROM json_each(p.manifest,'$.capabilities') WHERE value='sources'))")
            .bind(&self.installation).bind(&self.account).bind(&self.service).bind(&self.credential)
            .bind(self.generation)
            .bind(&self.source).bind(&self.remote_source).fetch_one(&self.store.pool)).await
            .map_err(|_| Error::Upstream("Audio authorization could not be checked in time"))??;
        if allowed {
            Ok(())
        } else {
            Err(Error::NotFound)
        }
    }
}
