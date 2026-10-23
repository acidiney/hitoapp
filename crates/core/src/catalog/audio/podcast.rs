use super::{
    AudioRequest, AudioStream, OpenedAudio, headers,
    origin::{AudioOrigin, Located},
    remote::{Authority, Permits, RemoteAudio},
};
use crate::{
    Actor, Error, PlaybackSource, Result, Store, admission::Accounts, catalog::push_source_choice,
    remote_http::RemoteHttp,
};
use futures_util::future::BoxFuture;
use sqlx::QueryBuilder;
use std::sync::Arc;
use tokio::sync::Semaphore;

pub(crate) struct EnclosureOrigin {
    pub store: Store,
    pub remote: RemoteHttp,
    pub slots: Arc<Semaphore>,
    pub accounts: Accounts,
}

#[derive(Clone, PartialEq, Eq, sqlx::FromRow)]
pub(crate) struct PodcastEnclosure {
    pub source_id: String,
    pub episode_id: String,
    pub recording_id: String,
    pub user_id: String,
    pub subscription_id: String,
    pub subscription_generation: i64,
    pub enclosure_revision: i64,
    pub enclosure_url: String,
    pub declared_mime: Option<String>,
    pub byte_length: Option<i64>,
}

impl EnclosureOrigin {
    async fn podcast_enclosure(
        &self,
        actor: &Actor,
        source: &str,
    ) -> Result<Option<PodcastEnclosure>> {
        let mut query = QueryBuilder::new(
            "SELECT enclosure.source_id,enclosure.episode_id,enclosure.recording_id,enclosure.user_id,
                enclosure.subscription_id,enclosure.subscription_generation,enclosure.enclosure_revision,
                enclosure.enclosure_url,enclosure.declared_mime,enclosure.byte_length
            FROM current_podcast_sources enclosure CROSS JOIN sources s ON s.id=enclosure.source_id
            WHERE enclosure.source_id=",
        );
        query
            .push_bind(source.to_owned())
            .push(" AND enclosure.user_id=")
            .push_bind(actor.id().to_owned())
            .push(" AND ");
        push_source_choice(&mut query, actor.id());
        Ok(query
            .build_query_as()
            .fetch_optional(&self.store.pool)
            .await?)
    }

    #[tracing::instrument(level = "debug", skip_all, fields(account_id = grant.actor().id(),
        source_id = %enclosure.source_id, episode_id = %enclosure.episode_id))]
    async fn open_enclosure(
        &self,
        grant: &PlaybackSource,
        enclosure: &PodcastEnclosure,
        request: AudioRequest,
    ) -> Result<OpenedAudio> {
        let account = self.accounts.enter(grant.actor().id())?;
        let global = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Conflict("Audio streams are busy. Try again.".into()))?;
        let remote = grant
            .while_authorized(self.remote.audio(&enclosure.enclosure_url, request))
            .await?;
        let mut opened = headers::read(&remote.response, request)?;
        tracing::debug!(
            event = "podcast_audio_opened",
            status = remote.response.status().as_u16(),
            enclosure_revision = enclosure.enclosure_revision,
            "Podcast audio headers verified"
        );
        if !request.head
            && let OpenedAudio::Content(audio) = &mut opened
        {
            audio.body = Some(AudioStream::remote(
                RemoteAudio::new(
                    remote.response,
                    Authority::Podcast(Box::new(grant.clone())),
                    Some(Permits {
                        _account: account,
                        _global: global,
                    }),
                ),
                audio.length,
            ));
        }
        Ok(opened)
    }
}

impl AudioOrigin for EnclosureOrigin {
    fn kind(&self) -> &'static str {
        "podcast"
    }

    fn locate<'a>(
        &'a self,
        actor: &'a Actor,
        source: &'a str,
    ) -> BoxFuture<'a, Result<Option<Located>>> {
        Box::pin(async move {
            Ok(self
                .podcast_enclosure(actor, source)
                .await?
                .map(Located::new))
        })
    }

    fn still_valid<'a>(
        &'a self,
        actor: &'a Actor,
        source: &'a str,
        located: &'a Located,
    ) -> BoxFuture<'a, Result<bool>> {
        Box::pin(async move {
            Ok(self.podcast_enclosure(actor, source).await?.as_ref()
                == Some(located.pinned::<PodcastEnclosure>()?))
        })
    }

    fn open<'a>(
        &'a self,
        grant: &'a PlaybackSource,
        request: AudioRequest,
    ) -> BoxFuture<'a, Result<OpenedAudio>> {
        Box::pin(async move {
            let enclosure = grant.located().pinned::<PodcastEnclosure>()?;
            self.open_enclosure(grant, enclosure, request).await
        })
    }
}
