use super::{Call, ControlClient};
use crate::{Error, Result};
use hito_plugin_sdk::{
    CATALOG_RELEASE_GROUP_CAPABILITIES_PATH, CATALOG_RELEASE_GROUP_PATH, CatalogReleaseGroup,
    CatalogReleaseGroupCapabilities, MAX_CATALOG_RELEASE_GROUP_BYTES, ReleaseGroupRequest,
};

impl ControlClient {
    pub(in crate::plugins) async fn catalog_release_group_supported_admitted(
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
                    path: CATALOG_RELEASE_GROUP_CAPABILITIES_PATH,
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
        let Ok(capabilities) = serde_json::from_value::<CatalogReleaseGroupCapabilities>(value)
        else {
            return Ok(false);
        };
        Ok(capabilities.validate().is_ok())
    }

    pub(in crate::plugins) async fn catalog_release_group_admitted(
        &self,
        base_url: &str,
        token: Option<&str>,
        request: &ReleaseGroupRequest,
    ) -> Result<CatalogReleaseGroup> {
        request
            .validate()
            .map_err(|_| Error::Invalid("Use a valid release-group request".into()))?;
        let body = serde_json::to_value(request).map_err(|_| {
            Error::Internal("The release-group request could not be encoded".into())
        })?;
        let mut remaining = MAX_CATALOG_RELEASE_GROUP_BYTES;
        let group: CatalogReleaseGroup = self
            .exchange_admitted(
                base_url,
                token,
                Call {
                    path: CATALOG_RELEASE_GROUP_PATH,
                    body: Some(body),
                    maximum: MAX_CATALOG_RELEASE_GROUP_BYTES,
                },
                &mut remaining,
            )
            .await?;
        group
            .validate_for(request)
            .map_err(|_| Error::Upstream("The provider returned an invalid release group"))?;
        Ok(group)
    }
}
