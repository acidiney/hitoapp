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
        let observation = self.credential_observation();
        let supported = self
            .remote
            .lyrics_supported_admitted(&self.base_url, self.token.as_deref().map(String::as_str))
            .await;
        check().await?;
        if !supported.map_err(|error| self.with_credential_context(error, &observation))? {
            return Ok(None);
        }
        let observation = self.credential_observation();
        let result = self
            .remote
            .lyrics_admitted(
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                request,
            )
            .await;
        #[cfg(feature = "test-support")]
        tracing::Instrument::instrument(
            check(),
            tracing::debug_span!(
                target: "hito_core::authority_snapshot_test",
                "lyric_legacy_document_postcheck"
            ),
        )
        .await?;
        #[cfg(not(feature = "test-support"))]
        check().await?;
        if result.is_ok() {
            self.credentials_verified(&observation);
        }
        result
            .map(Some)
            .map_err(|error| self.with_credential_context(error, &observation))
    }
}

pub(crate) enum LyricsDocumentResponse {
    V1(LyricsResponse),
    V2(hito_plugin_sdk::LyricsResponseV2),
}

#[derive(Clone, Copy)]
enum LyricProtocol {
    V1,
    V2,
    Unsupported,
}

pub(crate) struct LyricRequest<'a> {
    operation: &'a Operation,
    _admission: super::super::transport::Admission,
    protocol: Option<LyricProtocol>,
}

impl Operation {
    pub(crate) fn lyric_request(&self) -> Result<LyricRequest<'_>> {
        Ok(LyricRequest {
            operation: self,
            _admission: self.remote.admit_lyrics(&self.account)?,
            protocol: None,
        })
    }

    pub(crate) async fn selected_lyrics_checked<F, Check>(
        &self,
        request: &LyricsRequest,
        check: F,
    ) -> Result<Option<LyricsDocumentResponse>>
    where
        F: Fn() -> Check + Send + Sync,
        Check: Future<Output = Result<()>> + Send,
    {
        self.lyric_request()?.document_checked(request, check).await
    }

    pub(crate) async fn lyric_search_checked<F, Check>(
        &self,
        request: &hito_plugin_sdk::SearchRequest,
        check: F,
    ) -> Result<hito_plugin_sdk::SearchPage>
    where
        F: Fn() -> Check + Send + Sync,
        Check: Future<Output = Result<()>> + Send,
    {
        self.lyric_request()?.search_checked(request, check).await
    }

    pub(crate) async fn lyric_metadata_checked<F, Check>(
        &self,
        request: &hito_plugin_sdk::RecordingRequest,
        check: F,
    ) -> Result<hito_plugin_sdk::RecordingMetadata>
    where
        F: Fn() -> Check + Send + Sync,
        Check: Future<Output = Result<()>> + Send,
    {
        self.lyric_request()?.metadata_checked(request, check).await
    }
}

impl LyricRequest<'_> {
    pub(crate) async fn document_checked<F, Check>(
        &mut self,
        request: &LyricsRequest,
        check: F,
    ) -> Result<Option<LyricsDocumentResponse>>
    where
        F: Fn() -> Check + Send + Sync,
        Check: Future<Output = Result<()>> + Send,
    {
        check().await?;
        let protocol = match self.protocol {
            Some(protocol) => protocol,
            None => {
                let protocol = self.negotiate_checked(&check).await?;
                self.protocol = Some(protocol);
                protocol
            }
        };
        let remote = &self.operation.remote;
        let base = &self.operation.base_url;
        let token = self.operation.token.as_deref().map(String::as_str);
        let (result, observation) = match protocol {
            LyricProtocol::V2 => {
                let observation = self.operation.credential_observation();
                let result = remote
                    .lyrics_v2_admitted(base, token, request)
                    .await
                    .map(LyricsDocumentResponse::V2);
                (result, observation)
            }
            LyricProtocol::V1 => {
                let observation = self.operation.credential_observation();
                let result = remote
                    .lyrics_admitted(base, token, request)
                    .await
                    .map(LyricsDocumentResponse::V1);
                (result, observation)
            }
            LyricProtocol::Unsupported => return Ok(None),
        };
        #[cfg(feature = "test-support")]
        {
            use tracing::Instrument;
            check()
                .instrument(tracing::debug_span!(
                    target: "hito_core::authority_snapshot_test",
                    "lyric_document_postcheck"
                ))
                .await?;
        }
        #[cfg(not(feature = "test-support"))]
        check().await?;
        if result.is_ok() {
            self.operation.credentials_verified(&observation);
        }
        result
            .map(Some)
            .map_err(|error| self.operation.with_credential_context(error, &observation))
    }

    async fn negotiate_checked<F, Check>(&self, check: &F) -> Result<LyricProtocol>
    where
        F: Fn() -> Check + Send + Sync,
        Check: Future<Output = Result<()>> + Send,
    {
        let remote = &self.operation.remote;
        let base = &self.operation.base_url;
        let token = self.operation.token.as_deref().map(String::as_str);
        let observation = self.operation.credential_observation();
        let supported = remote.lyrics_v2_supported_admitted(base, token).await;
        check().await?;
        if supported.map_err(|error| self.operation.with_credential_context(error, &observation))? {
            return Ok(LyricProtocol::V2);
        }
        let observation = self.operation.credential_observation();
        let supported = remote.lyrics_supported_admitted(base, token).await;
        check().await?;
        Ok(
            if supported
                .map_err(|error| self.operation.with_credential_context(error, &observation))?
            {
                LyricProtocol::V1
            } else {
                LyricProtocol::Unsupported
            },
        )
    }
}

impl LyricRequest<'_> {
    pub(crate) async fn search_checked<F, Check>(
        &self,
        request: &hito_plugin_sdk::SearchRequest,
        check: F,
    ) -> Result<hito_plugin_sdk::SearchPage>
    where
        F: Fn() -> Check + Send + Sync,
        Check: Future<Output = Result<()>> + Send,
    {
        check().await?;
        let observation = self.operation.credential_observation();
        let result = self
            .operation
            .remote
            .search_admitted(
                &self.operation.base_url,
                self.operation.token.as_deref().map(String::as_str),
                request,
            )
            .await;
        #[cfg(feature = "test-support")]
        {
            use tracing::Instrument;
            check()
                .instrument(tracing::debug_span!(
                    target: "hito_core::authority_snapshot_test",
                    "lyric_search_postcheck"
                ))
                .await?;
        }
        #[cfg(not(feature = "test-support"))]
        check().await?;
        if result.is_ok() {
            self.operation
                .health
                .clear_credentials(&self.operation.connection_generation(), &observation);
        }
        result.map_err(|error| self.operation.with_credential_context(error, &observation))
    }

    pub(crate) async fn metadata_checked<F, Check>(
        &self,
        request: &hito_plugin_sdk::RecordingRequest,
        check: F,
    ) -> Result<hito_plugin_sdk::RecordingMetadata>
    where
        F: Fn() -> Check + Send + Sync,
        Check: Future<Output = Result<()>> + Send,
    {
        check().await?;
        let observation = self.operation.credential_observation();
        let result = self
            .operation
            .remote
            .recording_metadata_admitted(
                &self.operation.base_url,
                self.operation.token.as_deref().map(String::as_str),
                request,
            )
            .await;
        #[cfg(feature = "test-support")]
        tracing::Instrument::instrument(check(), tracing::debug_span!("lyric_metadata_postcheck"))
            .await?;
        #[cfg(not(feature = "test-support"))]
        check().await?;
        if result.is_ok() {
            self.operation.credentials_verified(&observation);
        }
        result.map_err(|error| self.operation.with_credential_context(error, &observation))
    }
}
