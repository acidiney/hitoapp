use super::Operation;
use crate::Result;
use hito_plugin_sdk::{CatalogReleaseGroup, ReleaseGroupRequest};

impl Operation {
    pub(crate) async fn catalog_release_group_supported(&self) -> Result<bool> {
        self.check_catalog().await?;
        let observation = self.credential_observation();
        let result = self
            .remote
            .catalog_release_group_supported_admitted(
                &self.base_url,
                self.token.as_deref().map(String::as_str),
            )
            .await;
        self.check_catalog().await?;
        result.map_err(|error| self.with_credential_context(error, &observation))
    }

    pub(crate) async fn catalog_release_group(
        &self,
        request: &ReleaseGroupRequest,
    ) -> Result<CatalogReleaseGroup> {
        self.check_catalog().await?;
        let observation = self.credential_observation();
        let result = self
            .remote
            .catalog_release_group_admitted(
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                request,
            )
            .await;
        self.check_catalog().await?;
        if result.is_ok() {
            self.credentials_verified(&observation);
        }
        result.map_err(|error| self.with_credential_context(error, &observation))
    }
}
