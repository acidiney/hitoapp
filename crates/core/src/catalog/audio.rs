mod enclosure;
pub(crate) mod headers;
mod range;
pub(crate) mod remote;
mod stream;
use super::Catalog;
use crate::{Actor, Result};
pub use range::ByteRange;
pub use stream::AudioStream;

#[derive(Default, Clone, Copy)]
pub struct AudioRequest {
    pub head: bool,
    pub range: Option<ByteRange>,
}

pub enum OpenedAudio {
    Content(Box<AudioContent>),
    Unsatisfiable { length: Option<u64> },
}

pub struct AudioContent {
    pub content_type: &'static str,
    pub length: Option<u64>,
    pub total_length: Option<u64>,
    pub range: Option<std::ops::RangeInclusive<u64>>,
    pub seekable: bool,
    pub body: Option<AudioStream>,
}

pub(super) struct ProviderAudio {
    pub operation: crate::plugins::Operation,
    pub remote: String,
    pub binding_id: String,
    pub binding_revision: i64,
}

#[derive(sqlx::FromRow)]
struct ProviderAudioBinding {
    installation_id: String,
    provider_source_id: String,
    installation_revision: i64,
    binding_id: String,
    binding_revision: i64,
}

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn open_audio(
        &self,
        actor: &Actor,
        source: &str,
        request: AudioRequest,
    ) -> Result<OpenedAudio> {
        if let Some(provider) = self.provider_audio(actor, source).await? {
            return provider
                .operation
                .audio(
                    source,
                    &provider.remote,
                    &provider.binding_id,
                    provider.binding_revision,
                    request,
                )
                .await;
        }
        let authority = self.audio_playback_source(actor, source).await?;
        if !authority.is_local() {
            return authority.open_podcast(request).await;
        }
        let audio = authority.open_file().await?;
        let total = audio.length;
        let range = if request.head {
            None
        } else if let Some(selection) = request.range {
            let Some(range) = selection.resolve(total) else {
                return Ok(OpenedAudio::Unsatisfiable {
                    length: Some(total),
                });
            };
            Some(range)
        } else {
            None
        };
        let start = range.as_ref().map_or(0, |range| *range.start());
        let length = range
            .as_ref()
            .map_or(total, |range| range.end() - range.start() + 1);
        let content_type = audio.content_type;
        let body = if request.head {
            None
        } else {
            Some(AudioStream::local(audio, authority, start, length).await?)
        };
        tracing::debug!(length, "Local audio opened");
        Ok(OpenedAudio::Content(Box::new(AudioContent {
            content_type,
            length: Some(length),
            total_length: Some(total),
            range,
            seekable: true,
            body,
        })))
    }

    pub(super) async fn provider_audio(
        &self,
        actor: &Actor,
        source: &str,
    ) -> Result<Option<ProviderAudio>> {
        let binding: Option<ProviderAudioBinding> = sqlx::query_as("SELECT ps.installation_id,ps.provider_source_id,p.revision AS installation_revision,item.id AS binding_id,item.revision AS binding_revision
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
        }))
    }
}
