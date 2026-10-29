use super::Operation;
use crate::Result;
use hito_plugin_sdk::{ArtistSimilarityPage, ArtistSimilarityRequest};

impl Operation {
    pub(crate) async fn catalog_similarity_supported(&self) -> Result<bool> {
        self.check_catalog().await?;
        let result = self
            .remote
            .catalog_similarity_supported_admitted(
                &self.base_url,
                self.token.as_deref().map(String::as_str),
            )
            .await;
        self.check_catalog().await?;
        result
    }

    pub(crate) async fn catalog_artist_similarity(
        &self,
        request: &ArtistSimilarityRequest,
    ) -> Result<ArtistSimilarityPage> {
        self.check_catalog().await?;
        let result = self
            .remote
            .catalog_artist_similarity_admitted(
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                request,
            )
            .await;
        self.check_catalog().await?;
        result
    }
}
