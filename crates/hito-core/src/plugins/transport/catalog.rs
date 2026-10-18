use super::{Call, ControlClient};
use crate::{Error, Result};
use hito_plugin_sdk::{
    ARTIST_PROFILE_PATH, ArtistProfileRequest, CATALOG_CAPABILITIES_PATH, CatalogArtistProfile,
    CatalogCapabilities, CatalogOperation, MAX_CATALOG_PROFILE_BYTES,
};

impl ControlClient {
    pub(in crate::plugins) async fn catalog_profile_supported_admitted(
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
                    path: CATALOG_CAPABILITIES_PATH,
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
        let Ok(capabilities) = serde_json::from_value::<CatalogCapabilities>(value) else {
            return Ok(false);
        };
        Ok(capabilities.validate().is_ok()
            && capabilities
                .operations
                .contains(&CatalogOperation::ArtistProfile))
    }

    pub(in crate::plugins) async fn catalog_artist_profile_admitted(
        &self,
        base_url: &str,
        token: Option<&str>,
        request: &ArtistProfileRequest,
    ) -> Result<CatalogArtistProfile> {
        request
            .validate()
            .map_err(|_| Error::Invalid("Use a valid provider artist identifier".into()))?;
        let body = serde_json::to_value(request).map_err(|_| {
            Error::Internal("The artist profile request could not be encoded".into())
        })?;
        let mut remaining = MAX_CATALOG_PROFILE_BYTES;
        let profile: CatalogArtistProfile = self
            .exchange_admitted(
                base_url,
                token,
                Call {
                    path: ARTIST_PROFILE_PATH,
                    body: Some(body),
                    maximum: MAX_CATALOG_PROFILE_BYTES,
                },
                &mut remaining,
            )
            .await?;
        profile
            .validate_for(request)
            .map_err(|_| Error::Upstream("The provider returned an invalid artist profile"))?;
        Ok(profile)
    }
}
