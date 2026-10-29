use super::{Call, ControlClient};
use crate::{Error, Result};
use hito_plugin_sdk::{
    ARTIST_SIMILARITY_PATH, ArtistSimilarityPage, ArtistSimilarityRequest,
    CATALOG_SIMILARITY_CAPABILITIES_PATH, CatalogSimilarityCapabilities, CatalogSimilarityIdentity,
    CatalogSimilarityOperation, MAX_SIMILAR_ARTISTS_BYTES,
};

impl ControlClient {
    pub(in crate::plugins) async fn catalog_similarity_supported_admitted(
        &self,
        base_url: &str,
        token: Option<&str>,
    ) -> Result<bool> {
        let mut remaining = 4096;
        let value: Option<serde_json::Value> = self
            .exchange_optional_admitted(
                base_url,
                token,
                Call {
                    path: CATALOG_SIMILARITY_CAPABILITIES_PATH,
                    body: None,
                    maximum: 4096,
                },
                &mut remaining,
                true,
            )
            .await?;
        let Some(value) = value else {
            return Ok(false);
        };
        let Ok(capabilities) = serde_json::from_value::<CatalogSimilarityCapabilities>(value)
        else {
            return Ok(false);
        };
        Ok(capabilities.validate().is_ok()
            && capabilities.operations == [CatalogSimilarityOperation::ArtistSimilarity]
            && capabilities.artist_identity == CatalogSimilarityIdentity::MusicbrainzArtist)
    }

    pub(in crate::plugins) async fn catalog_artist_similarity_admitted(
        &self,
        base_url: &str,
        token: Option<&str>,
        request: &ArtistSimilarityRequest,
    ) -> Result<ArtistSimilarityPage> {
        request
            .validate()
            .map_err(|_| Error::Invalid("Use a valid similar-artist request".into()))?;
        let body = serde_json::to_value(request).map_err(|_| {
            Error::Internal("The similar-artist request could not be encoded".into())
        })?;
        let mut remaining = MAX_SIMILAR_ARTISTS_BYTES;
        let page: ArtistSimilarityPage = self
            .exchange_admitted(
                base_url,
                token,
                Call {
                    path: ARTIST_SIMILARITY_PATH,
                    body: Some(body),
                    maximum: MAX_SIMILAR_ARTISTS_BYTES,
                },
                &mut remaining,
            )
            .await?;
        page.validate_for(request)
            .map_err(|_| Error::Upstream("The provider returned invalid similar artists"))?;
        Ok(page)
    }
}
