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

pub(crate) enum LyricsDocumentResponse {
    V1(LyricsResponse),
    V2(hito_plugin_sdk::LyricsResponseV2),
}

pub(crate) struct LyricRequest<'a> {
    operation: &'a Operation,
    _admission: super::super::transport::Admission,
}

impl Operation {
    pub(crate) fn lyric_request(&self) -> Result<LyricRequest<'_>> {
        Ok(LyricRequest {
            operation: self,
            _admission: self.remote.admit_lyrics(&self.account)?,
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
        &self,
        request: &LyricsRequest,
        check: F,
    ) -> Result<Option<LyricsDocumentResponse>>
    where
        F: Fn() -> Check + Send + Sync,
        Check: Future<Output = Result<()>> + Send,
    {
        check().await?;
        let supported = self
            .operation
            .remote
            .lyrics_v2_supported_admitted(
                &self.operation.base_url,
                self.operation.token.as_deref().map(String::as_str),
            )
            .await;
        check().await?;
        if supported? {
            let result = self
                .operation
                .remote
                .lyrics_v2_admitted(
                    &self.operation.base_url,
                    self.operation.token.as_deref().map(String::as_str),
                    request,
                )
                .await;
            check().await?;
            return result.map(|document| Some(LyricsDocumentResponse::V2(document)));
        }
        let supported = self
            .operation
            .remote
            .lyrics_supported_admitted(
                &self.operation.base_url,
                self.operation.token.as_deref().map(String::as_str),
            )
            .await;
        check().await?;
        if !supported? {
            return Ok(None);
        }
        let result = self
            .operation
            .remote
            .lyrics_admitted(
                &self.operation.base_url,
                self.operation.token.as_deref().map(String::as_str),
                request,
            )
            .await;
        check().await?;
        result.map(|document| Some(LyricsDocumentResponse::V1(document)))
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
        let result = self
            .operation
            .remote
            .search_admitted(
                &self.operation.base_url,
                self.operation.token.as_deref().map(String::as_str),
                request,
            )
            .await;
        check().await?;
        result
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
        let result = self
            .operation
            .remote
            .recording_metadata_admitted(
                &self.operation.base_url,
                self.operation.token.as_deref().map(String::as_str),
                request,
            )
            .await;
        check().await?;
        result
    }
}
