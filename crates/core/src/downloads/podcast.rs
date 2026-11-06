use super::{Downloads, lease::Lease, origin::DownloadOrigin};
use crate::{
    AudioRequest, AudioStream, OpenedAudio, Result,
    playback_access::relay::{
        headers,
        remote::{Authority, RemoteAudio},
    },
};

impl Downloads {
    #[tracing::instrument(level = "debug", skip_all,
        fields(job_id = %lease.id, intent_id = %lease.intent_id, account_id = %lease.account_id))]
    pub(super) async fn open_origin(
        &self,
        lease: &Lease,
        request: AudioRequest,
    ) -> Result<OpenedAudio> {
        match &lease.origin {
            DownloadOrigin::Provider(origin) => {
                self.plugins
                    .open_download(&lease.account_id, origin, request)
                    .await
            }
            DownloadOrigin::Podcast(origin) => {
                lease.check(&self.store).await?;
                let remote = self
                    .remote
                    .audio(&origin.podcast_enclosure_url, request)
                    .await?;
                lease.check(&self.store).await?;
                let mut opened = headers::read(&remote.response, request)?;
                tracing::debug!(
                    event = "podcast_download_audio_opened",
                    status = remote.response.status().as_u16(),
                    enclosure_revision = origin.podcast_enclosure_revision,
                    "Podcast download headers verified"
                );
                if let OpenedAudio::Content(audio) = &mut opened {
                    audio.body = Some(AudioStream::remote(
                        RemoteAudio::new(
                            remote.response,
                            Authority::new(lease.authority(&self.store)),
                            None,
                            crate::StreamPace::DEFAULT,
                        ),
                        audio.length,
                    ));
                }
                Ok(opened)
            }
        }
    }
}
