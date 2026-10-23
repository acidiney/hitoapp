use super::{
    AudioRequest, AudioStream, OpenedAudio, headers,
    remote::{Authority, Permits, RemoteAudio},
};
use crate::{Catalog, Error, PlaybackSource, Result, catalog::audio::podcast::PodcastEnclosure};

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, fields(account_id = account,
        source_id = %enclosure.source_id, episode_id = %enclosure.episode_id))]
    pub(in crate::catalog) async fn open_podcast_audio(
        &self,
        account: &str,
        enclosure: &PodcastEnclosure,
        authority: PlaybackSource,
        request: AudioRequest,
    ) -> Result<OpenedAudio> {
        let account = self.relay.podcast_accounts.enter(account)?;
        let global = self
            .relay
            .podcast_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Conflict("Audio streams are busy. Try again.".into()))?;
        let remote = authority
            .while_authorized(
                self.relay
                    .podcast_remote
                    .audio(&enclosure.enclosure_url, request),
            )
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
                    Authority::Podcast(Box::new(authority)),
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
