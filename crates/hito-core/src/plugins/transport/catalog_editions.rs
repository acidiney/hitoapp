use super::{Call, ControlClient};
use crate::{Error, Result};
use hito_plugin_sdk::{
    CATALOG_EDITION_CAPABILITIES_PATH, CatalogEditionCapabilities, CatalogEditionOperation,
    CatalogEditionTrackList, EDITION_TRACK_LIST_PATH, EditionTrackListRequest,
    MAX_CATALOG_EDITION_BYTES, MAX_CATALOG_PAGE_BYTES, RELEASE_GROUP_EDITIONS_PATH,
    ReleaseGroupEditionsPage, ReleaseGroupEditionsRequest,
};

impl ControlClient {
    pub(in crate::plugins) async fn catalog_edition_supported_admitted(
        &self,
        base_url: &str,
        token: Option<&str>,
        operation: CatalogEditionOperation,
    ) -> Result<bool> {
        let mut remaining = 4096;
        let value: Option<serde_json::Value> = self
            .exchange_optional_admitted(
                base_url,
                token,
                Call {
                    path: CATALOG_EDITION_CAPABILITIES_PATH,
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
        let Ok(capabilities) = serde_json::from_value::<CatalogEditionCapabilities>(value) else {
            return Ok(false);
        };
        Ok(capabilities.validate().is_ok() && capabilities.operations.contains(&operation))
    }

    pub(in crate::plugins) async fn catalog_release_group_editions_admitted(
        &self,
        base_url: &str,
        token: Option<&str>,
        request: &ReleaseGroupEditionsRequest,
    ) -> Result<ReleaseGroupEditionsPage> {
        request
            .validate()
            .map_err(|_| Error::Invalid("Use a valid release-group edition request".into()))?;
        let body = serde_json::to_value(request).map_err(|_| {
            Error::Internal("The release-group edition request could not be encoded".into())
        })?;
        let mut remaining = MAX_CATALOG_PAGE_BYTES;
        let page: ReleaseGroupEditionsPage = self
            .exchange_admitted(
                base_url,
                token,
                Call {
                    path: RELEASE_GROUP_EDITIONS_PATH,
                    body: Some(body),
                    maximum: MAX_CATALOG_PAGE_BYTES,
                },
                &mut remaining,
            )
            .await?;
        page.validate_for(request)
            .map_err(|_| Error::Upstream("The provider returned invalid release-group editions"))?;
        Ok(page)
    }

    pub(in crate::plugins) async fn catalog_edition_track_list_admitted(
        &self,
        base_url: &str,
        token: Option<&str>,
        request: &EditionTrackListRequest,
    ) -> Result<CatalogEditionTrackList> {
        request
            .validate()
            .map_err(|_| Error::Invalid("Use a valid edition track-list request".into()))?;
        let body = serde_json::to_value(request).map_err(|_| {
            Error::Internal("The edition track-list request could not be encoded".into())
        })?;
        let mut remaining = MAX_CATALOG_EDITION_BYTES;
        let edition: CatalogEditionTrackList = self
            .exchange_admitted(
                base_url,
                token,
                Call {
                    path: EDITION_TRACK_LIST_PATH,
                    body: Some(body),
                    maximum: MAX_CATALOG_EDITION_BYTES,
                },
                &mut remaining,
            )
            .await?;
        edition
            .validate_for(request)
            .map_err(|_| Error::Upstream("The provider returned an invalid edition track list"))?;
        Ok(edition)
    }
}
