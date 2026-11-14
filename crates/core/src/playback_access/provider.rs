use super::origin::{AudioOrigin, Located};
use crate::{Actor, AudioRequest, Error, OpenedAudio, PlaybackSource, Result, Store};
use futures_util::future::BoxFuture;

pub(crate) struct ProviderOrigin {
    pub store: Store,
    pub plugins: crate::Plugins,
}

struct ProviderAudio {
    operation: crate::plugins::Operation,
    remote: String,
    binding_id: String,
    binding_revision: i64,
    source_generation: i64,
}

#[derive(sqlx::FromRow)]
struct ProviderAudioBinding {
    installation_id: String,
    provider_source_id: String,
    installation_revision: i64,
    binding_id: String,
    binding_revision: i64,
    source_generation: i64,
}

impl ProviderOrigin {
    pub async fn open_current(
        &self,
        actor: &Actor,
        source: &str,
        request: AudioRequest,
    ) -> Result<Option<OpenedAudio>> {
        let Some(provider) = self.provider_audio(actor, source).await? else {
            return Ok(None);
        };
        provider
            .operation
            .audio(
                source,
                &provider.remote,
                &provider.binding_id,
                provider.binding_revision,
                provider.source_generation,
                request,
            )
            .await
            .map(Some)
    }

    async fn provider_audio(&self, actor: &Actor, source: &str) -> Result<Option<ProviderAudio>> {
        let binding: Option<ProviderAudioBinding> = sqlx::query_as("SELECT ps.installation_id,ps.provider_source_id,p.revision AS installation_revision,item.id AS binding_id,item.revision AS binding_revision,ps.generation AS source_generation
            FROM provider_sources ps JOIN sources s ON s.id=ps.source_id AND s.ready=1
            JOIN provider_items item ON item.id=ps.binding_id AND item.retired=0
            JOIN provider_source_state state ON state.binding_id=item.id AND state.generation=ps.generation
            JOIN recordings r ON r.id=s.recording_id JOIN library_members m ON m.library_id=r.library_id
            JOIN plugin_installations p ON p.id=ps.installation_id AND p.user_id=m.user_id AND p.active=1
            JOIN plugin_services service ON service.id=p.service_id AND service.active=1
            JOIN source_choice_bindings permitted ON permitted.binding_id=item.id AND permitted.recording_id=s.recording_id AND permitted.user_id=p.user_id
            WHERE ps.source_id=? AND m.user_id=?")
            .bind(source).bind(actor.id()).fetch_optional(&self.store.pool).await?;
        let Some(binding) = binding else {
            return Ok(None);
        };
        let operation = self
            .plugins
            .operation(
                actor,
                &binding.installation_id,
                binding.installation_revision,
                &[hito_plugin_sdk::Capability::Sources],
            )
            .await?;
        Ok(Some(ProviderAudio {
            operation,
            remote: binding.provider_source_id,
            binding_id: binding.binding_id,
            binding_revision: binding.binding_revision,
            source_generation: binding.source_generation,
        }))
    }
}

impl AudioOrigin for ProviderOrigin {
    fn kind(&self) -> &'static str {
        "provider"
    }

    fn locate<'a>(
        &'a self,
        actor: &'a Actor,
        source: &'a str,
    ) -> BoxFuture<'a, Result<Option<Located>>> {
        Box::pin(async move {
            Ok(self.provider_audio(actor, source).await?.map(|provider| {
                Located::new(provider.operation.audio_authority(
                    source,
                    &provider.remote,
                    &provider.binding_id,
                    provider.binding_revision,
                    provider.source_generation,
                ))
            }))
        })
    }

    fn still_valid<'a>(
        &'a self,
        _actor: &'a Actor,
        _source: &'a str,
        located: &'a Located,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            located
                .pinned::<crate::plugins::AudioAuthority>()?
                .check()
                .await?;
            Ok(true)
        })
    }

    fn warm_still_valid<'a>(
        &'a self,
        actor: &'a Actor,
        source: &'a str,
        located: &'a Located,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            let pinned = located.pinned::<crate::plugins::AudioAuthority>()?;
            pinned.check().await?;
            let Some(provider) = self.provider_audio(actor, source).await? else {
                return Ok(false);
            };
            let current = provider.operation.audio_authority(
                source,
                &provider.remote,
                &provider.binding_id,
                provider.binding_revision,
                provider.source_generation,
            );
            Ok(current.same_identity(pinned))
        })
    }

    fn open<'a>(
        &'a self,
        grant: &'a PlaybackSource,
        request: AudioRequest,
    ) -> BoxFuture<'a, Result<OpenedAudio>> {
        Box::pin(grant.supervised(async move {
            self.open_current(grant.actor(), grant.source_id(), request)
                .await?
                .ok_or(Error::NotFound)
        }))
    }

    fn open_warm<'a>(
        &'a self,
        grant: &'a PlaybackSource,
        request: AudioRequest,
    ) -> BoxFuture<'a, Result<Option<(OpenedAudio, std::sync::Arc<crate::plugins::WarmTicket>)>>> {
        Box::pin(async move {
            grant.warm_check().await?;
            let Some(provider) = self.provider_audio(grant.actor(), grant.source_id()).await? else {
                return Ok(None);
            };
            let located = Located::new(provider.operation.audio_authority(
                grant.source_id(),
                &provider.remote,
                &provider.binding_id,
                provider.binding_revision,
                provider.source_generation,
            ));
            let current = located.pinned::<crate::plugins::AudioAuthority>()?;
            let pinned = grant.located().pinned::<crate::plugins::AudioAuthority>()?;
            if !current.same_identity(pinned) {
                return Ok(None);
            }
            let mut opened = provider
                .operation
                .audio_warm(
                    grant.source_id(),
                    &provider.remote,
                    &provider.binding_id,
                    provider.binding_revision,
                    provider.source_generation,
                    request,
                )
                .await?;
            if let Err(error) = grant.warm_check().await {
                shutdown_audio(&mut opened.0).await;
                return Err(error);
            }
            Ok(Some(opened))
        })
    }
}

async fn shutdown_audio(opened: &mut OpenedAudio) {
    if let OpenedAudio::Content(content) = opened
        && let Some(body) = &mut content.body
    {
        body.shutdown().await;
    }
}
