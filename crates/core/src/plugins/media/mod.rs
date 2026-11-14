mod authority;
use crate::{
    AudioRequest, AudioStream, Error, OpenedAudio, PlaybackFailureReason, Result,
    admission::Accounts,
    playback_access::relay::{
        headers,
        remote::{Authority as StreamAuthority, Permits, RemoteAudio},
    },
};
pub(crate) use authority::Authority;
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;

const ACCOUNT_ADMISSION_WAIT: Duration = Duration::from_millis(500);

#[derive(Clone)]
pub(super) struct MediaClient {
    client: reqwest::Client,
    slots: Arc<Semaphore>,
    accounts: Accounts,
    pace: crate::StreamPace,
}

impl MediaClient {
    pub(super) fn new(pace: crate::StreamPace) -> Result<Self> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(2))
            .http1_only()
            .http1_max_headers(32)
            .pool_max_idle_per_host(2)
            .retry(reqwest::retry::never())
            .user_agent(concat!("Hito/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| Error::Internal("Audio HTTP transport could not be initialized".into()))?;
        Ok(Self {
            client,
            slots: Arc::new(Semaphore::new(16)),
            accounts: Accounts::new(2),
            pace,
        })
    }
    #[tracing::instrument(level = "debug", skip_all)]
    pub(super) async fn open(
        &self,
        base_url: &str,
        token: Option<&str>,
        request: AudioRequest,
        authority: Authority,
    ) -> Result<OpenedAudio> {
        let account = self
            .accounts
            .enter_within(&authority.account, ACCOUNT_ADMISSION_WAIT)
            .await?;
        let global = self
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Conflict("Audio streams are busy. Try again.".into()))?;
        let mut endpoint = super::offers::endpoint(base_url)?
            .join(hito_plugin_sdk::AUDIO_PATH)
            .map_err(|_| Error::Invalid("Invalid plugin service address".into()))?;
        endpoint
            .query_pairs_mut()
            .append_pair("source_id", &authority.remote_source);
        let mut outgoing = self
            .client
            .request(
                if request.head {
                    reqwest::Method::HEAD
                } else {
                    reqwest::Method::GET
                },
                endpoint,
            )
            .header("accept", "audio/*, application/ogg")
            .header("accept-encoding", "identity")
            .header("x-hito-plugin-protocol", "1");
        if !request.head
            && let Some(range) = request.range
        {
            outgoing = outgoing.header("range", range.header());
        }
        if let Some(token) = token {
            super::vault::validate(token)?;
            outgoing = outgoing.bearer_auth(token);
        }
        let response = authority
            .while_valid(async {
                tokio::time::timeout(Duration::from_secs(10), outgoing.send())
                    .await
                    .map_err(|_| {
                        playback_error(
                            Error::Upstream("The audio service did not return headers in time"),
                            PlaybackFailureReason::ProviderUnavailable,
                        )
                    })?
                    .map_err(|_| {
                        playback_error(
                            Error::Upstream("The audio service could not be reached"),
                            PlaybackFailureReason::ProviderUnavailable,
                        )
                    })
            })
            .await?;
        tracing::debug!(
            status = response.status().as_u16(),
            "Provider audio headers received"
        );
        let status = response.status();
        if !matches!(
            status,
            reqwest::StatusCode::OK
                | reqwest::StatusCode::PARTIAL_CONTENT
                | reqwest::StatusCode::RANGE_NOT_SATISFIABLE
        ) {
            let reason = match status.as_u16() {
                401 => PlaybackFailureReason::CredentialsRequired,
                408 | 429 | 500 | 502 | 503 | 504 => PlaybackFailureReason::ProviderUnavailable,
                400..=499 | 501 | 505 => PlaybackFailureReason::ProviderRefused,
                _ => PlaybackFailureReason::InvalidProviderResponse,
            };
            return Err(playback_error(
                Error::Upstream("The audio service returned an unsuccessful status"),
                reason,
            ));
        }
        let mut opened = headers::read(&response, request).map_err(|error| {
            playback_error(error, PlaybackFailureReason::InvalidProviderResponse)
        })?;
        authority.check().await?;
        if !request.head
            && let OpenedAudio::Content(audio) = &mut opened
        {
            audio.body = Some(AudioStream::remote(
                RemoteAudio::new(
                    response,
                    StreamAuthority::new(authority),
                    Some(Permits {
                        _account: account,
                        _global: global,
                    }),
                    self.pace,
                ),
                audio.length,
            ));
        }
        Ok(opened)
    }
}

fn playback_error(error: Error, reason: PlaybackFailureReason) -> Error {
    Error::PlaybackProvider {
        reason,
        original: Box::new(error),
    }
}
