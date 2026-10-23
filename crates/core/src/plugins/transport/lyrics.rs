use super::{Admission, Call, ControlClient};
use crate::{Error, Result};
use hito_plugin_sdk::{
    LYRICS_CAPABILITIES_PATH, LYRICS_PATH, LyricsCapabilities, LyricsRequest, LyricsResponse,
    MAX_LYRICS_RESPONSE_BYTES,
};

impl ControlClient {
    pub(in crate::plugins) fn admit_lyrics(&self, account: &str) -> Result<Admission> {
        let account = self.lyrics_accounts.enter(account)?;
        let global = self
            .lyrics_slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| Error::Conflict("Lyric requests are busy. Try again.".into()))?;
        Ok(Admission {
            _account: account,
            _global: global,
        })
    }

    pub(in crate::plugins) async fn lyrics_supported_admitted(
        &self,
        base_url: &str,
        token: Option<&str>,
    ) -> Result<bool> {
        let mut remaining = 4096;
        let descriptor: Option<serde_json::Value> = self
            .exchange_optional_admitted(
                base_url,
                token,
                Call {
                    path: LYRICS_CAPABILITIES_PATH,
                    body: None,
                    maximum: 4096,
                },
                &mut remaining,
                true,
            )
            .await?;
        let Some(descriptor) = descriptor else {
            return Ok(false);
        };
        let Ok(capabilities) = serde_json::from_value::<LyricsCapabilities>(descriptor) else {
            return Ok(false);
        };
        Ok(capabilities.validate().is_ok())
    }

    pub(in crate::plugins) async fn lyrics_admitted(
        &self,
        base_url: &str,
        token: Option<&str>,
        request: &LyricsRequest,
    ) -> Result<LyricsResponse> {
        request
            .validate()
            .map_err(|_| Error::Invalid("Use a valid retained provider identity".into()))?;
        let body = serde_json::to_value(request)
            .map_err(|_| Error::Internal("The lyric request could not be encoded".into()))?;
        let mut remaining = MAX_LYRICS_RESPONSE_BYTES;
        let response: LyricsResponse = self
            .exchange_admitted(
                base_url,
                token,
                Call {
                    path: LYRICS_PATH,
                    body: Some(body),
                    maximum: MAX_LYRICS_RESPONSE_BYTES,
                },
                &mut remaining,
            )
            .await?;
        response
            .validate_for(request)
            .map_err(|_| Error::Upstream("The provider returned an invalid lyric response"))?;
        Ok(response)
    }
}
