use super::Operation;
use crate::Result;
use hito_plugin_sdk::{ArtistSearchPage, ArtistSearchRequest};

pub(crate) struct CatalogDiscoveryResponse<T> {
    pub(crate) result: Result<T>,
    pub(crate) observation: Option<super::super::health::CredentialTicket>,
}

impl Operation {
    pub(crate) async fn catalog_discovery_supported(
        &self,
        observe_credentials: bool,
    ) -> CatalogDiscoveryResponse<bool> {
        let mut observation = None;
        let result = async {
            self.check_catalog().await?;
            observation = observe_credentials.then(|| self.credential_observation());
            let result = self
                .remote
                .catalog_discovery_supported_admitted(
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

    pub(crate) async fn catalog_artist_search(
        &self,
        request: &ArtistSearchRequest,
        observe_credentials: bool,
    ) -> CatalogDiscoveryResponse<ArtistSearchPage> {
        #[cfg(feature = "test-support")]
        tracing::debug!(
            target: "hito_core::catalog_dispatch_test",
            point = "artist_search_before_preflight"
        );
        let mut observation = None;
        let result = async {
            self.check_catalog().await?;
            observation = observe_credentials.then(|| self.credential_observation());
            let result = self
                .remote
                .catalog_artist_search_admitted(
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
