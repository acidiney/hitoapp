use super::{
    PluginInstallationPreview, Plugins, PreviewPluginInstallation,
    installations::{check_capacity, offered_service},
    manifest::fingerprint,
};
use crate::{Actor, Result};
use std::time::Instant;

impl Plugins {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn preview_installation(
        &self,
        actor: &Actor,
        request: PreviewPluginInstallation,
    ) -> Result<PluginInstallationPreview> {
        let started = Instant::now();
        let result: Result<PluginInstallationPreview> = async {
            let service = {
                let mut connection = self.store.pool.acquire().await?;
                actor.check_session_on(&mut connection).await?;
                let service = offered_service(
                    &mut connection,
                    &request.service_id,
                    request.service_revision,
                )
                .await?;
                check_capacity(&mut connection, actor.id(), &service.id).await?;
                service
            };
            let manifest = self
                .remote
                .manifest(
                    actor.id(),
                    &service.base_url,
                    request.bearer_token.as_deref(),
                )
                .await?;
            let (_, digest) = fingerprint(&manifest)?;
            let mut tx = self.store.pool.begin().await?;
            actor.check_session_on(&mut tx).await?;
            offered_service(&mut tx, &service.id, service.revision).await?;
            check_capacity(&mut tx, actor.id(), &service.id).await?;
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            Ok(PluginInstallationPreview { manifest, digest })
        }
        .await;
        tracing::info!(
            outcome = result
                .as_ref()
                .map(|_| "previewed")
                .unwrap_or_else(|error| error.code()),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Plugin installation preview completed"
        );
        result
    }
}
