mod range;
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

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn open_audio(
        &self,
        actor: &Actor,
        source: &str,
        request: AudioRequest,
    ) -> Result<OpenedAudio> {
        let binding: Option<(String,String,i64)> = sqlx::query_as("SELECT ps.installation_id,ps.provider_source_id,p.revision
            FROM provider_sources ps JOIN sources s ON s.id=ps.source_id AND s.ready=1
            JOIN recordings r ON r.id=s.recording_id JOIN library_members m ON m.library_id=r.library_id
            JOIN plugin_installations p ON p.id=ps.installation_id AND p.user_id=m.user_id AND p.active=1
            JOIN plugin_services service ON service.id=p.service_id AND service.active=1
            WHERE ps.source_id=? AND m.user_id=?")
            .bind(source).bind(actor.id()).fetch_optional(&self.store.pool).await?;
        if let Some((installation, remote, revision)) = binding {
            let operation = self
                .plugins
                .operation(
                    actor,
                    &installation,
                    revision,
                    &[hito_plugin_sdk::Capability::Sources],
                )
                .await?;
            return operation.audio(source, &remote, request).await;
        }
        let audio = self.audio_file(actor, source).await?;
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
            Some(AudioStream::local(audio, start, length).await?)
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
}
