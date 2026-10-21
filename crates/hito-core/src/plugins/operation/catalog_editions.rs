use super::Operation;
use crate::Result;
use hito_plugin_sdk::{
    CatalogEditionOperation, CatalogEditionTrackList, EditionTrackListRequest,
    ReleaseGroupEditionsPage, ReleaseGroupEditionsRequest,
};

impl Operation {
    pub(crate) async fn catalog_edition_supported(
        &self,
        operation: CatalogEditionOperation,
    ) -> Result<bool> {
        self.check_catalog().await?;
        let result = self
            .remote
            .catalog_edition_supported_admitted(
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                operation,
            )
            .await;
        self.check_catalog().await?;
        result
    }

    pub(crate) async fn catalog_release_group_editions(
        &self,
        request: &ReleaseGroupEditionsRequest,
    ) -> Result<ReleaseGroupEditionsPage> {
        self.check_catalog().await?;
        let result = self
            .remote
            .catalog_release_group_editions_admitted(
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                request,
            )
            .await;
        self.check_catalog().await?;
        result
    }

    pub(crate) async fn catalog_edition_track_list(
        &self,
        request: &EditionTrackListRequest,
    ) -> Result<CatalogEditionTrackList> {
        self.check_catalog().await?;
        let result = self
            .remote
            .catalog_edition_track_list_admitted(
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                request,
            )
            .await;
        self.check_catalog().await?;
        result
    }
}
