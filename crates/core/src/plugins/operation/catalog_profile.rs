use super::{CatalogDiscoveryResponse, Operation};
use crate::Result;
use hito_plugin_sdk::{ArtistProfileRequest, CatalogArtistProfile};

impl Operation {
    pub(crate) async fn catalog_profile_supported(&self) -> Result<bool> {
        self.catalog_profile_supported_checked(false).await.result
    }

    pub(crate) async fn catalog_artist_profile(
        &self,
        request: &ArtistProfileRequest,
    ) -> Result<CatalogArtistProfile> {
        self.catalog_artist_profile_checked(request, false)
            .await
            .result
    }

    pub(crate) async fn catalog_profile_supported_checked(
        &self,
        observe_credentials: bool,
    ) -> CatalogDiscoveryResponse<bool> {
        let mut observation = None;
        let result = async {
            self.check_catalog().await?;
            observation = observe_credentials.then(|| self.credential_observation());
            let result = self
                .remote
                .catalog_profile_supported_admitted(
                    &self.base_url,
                    self.token.as_deref().map(String::as_str),
                )
                .await;
            self.check_catalog().await?;
            result
        }
        .await;
        CatalogDiscoveryResponse {
            result,
            observation,
        }
    }

    pub(crate) async fn catalog_artist_profile_checked(
        &self,
        request: &ArtistProfileRequest,
        observe_credentials: bool,
    ) -> CatalogDiscoveryResponse<CatalogArtistProfile> {
        let mut observation = None;
        let result = async {
            self.check_catalog().await?;
            observation = observe_credentials.then(|| self.credential_observation());
            let result = self
                .remote
                .catalog_artist_profile_admitted(
                    &self.base_url,
                    self.token.as_deref().map(String::as_str),
                    request,
                )
                .await;
            self.check_catalog().await?;
            result
        }
        .await;
        CatalogDiscoveryResponse {
            result,
            observation,
        }
    }
}
