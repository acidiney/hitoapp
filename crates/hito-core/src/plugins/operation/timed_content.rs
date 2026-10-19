use super::Operation;
use crate::Result;
use hito_plugin_sdk::{LyricsRequest, LyricsResponse};
use std::future::Future;

impl Operation {
    pub(crate) async fn lyrics_checked<F, Check>(
        &self,
        request: &LyricsRequest,
        check: F,
    ) -> Result<Option<LyricsResponse>>
    where
        F: Fn() -> Check + Send + Sync,
        Check: Future<Output = Result<()>> + Send,
    {
        let _admission = self.remote.admit_lyrics(&self.account)?;
        check().await?;
        let supported = self
            .remote
            .lyrics_supported_admitted(&self.base_url, self.token.as_deref().map(String::as_str))
            .await;
        check().await?;
        if !supported? {
            return Ok(None);
        }
        let result = self
            .remote
            .lyrics_admitted(
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                request,
            )
            .await;
        check().await?;
        result.map(Some)
    }
}
