use super::{Call, ControlClient};
use crate::{Error, Result};
use hito_plugin_sdk::{
    ARTIST_DISCOGRAPHY_PATH, ARTIST_PROFILE_PATH, ARTIST_SEARCH_PATH, ArtistDiscographyPage,
    ArtistDiscographyRequest, ArtistProfileRequest, ArtistSearchPage, ArtistSearchRequest,
    CATALOG_CAPABILITIES_PATH, CATALOG_DISCOGRAPHY_CAPABILITIES_PATH,
    CATALOG_DISCOVERY_CAPABILITIES_PATH, CatalogArtistProfile, CatalogCapabilities,
    CatalogDiscographyCapabilities, CatalogDiscographyOperation, CatalogDiscoveryCapabilities,
    CatalogDiscoveryOperation, CatalogOperation, MAX_ARTIST_SEARCH_PAGE_BYTES,
    MAX_CATALOG_PAGE_BYTES, MAX_CATALOG_PROFILE_BYTES,
};

impl ControlClient {
    pub(in crate::plugins) async fn catalog_discovery_supported_admitted(
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
                    path: CATALOG_DISCOVERY_CAPABILITIES_PATH,
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
        let Ok(capabilities) = serde_json::from_value::<CatalogDiscoveryCapabilities>(value) else {
            return Ok(false);
        };
        Ok(capabilities.validate().is_ok()
            && capabilities
                .operations
                .contains(&CatalogDiscoveryOperation::ArtistSearch))
    }

    pub(in crate::plugins) async fn catalog_artist_search_admitted(
        &self,
        base_url: &str,
        token: Option<&str>,
        request: &ArtistSearchRequest,
    ) -> Result<ArtistSearchPage> {
        request
            .validate()
            .map_err(|_| Error::Invalid("Use a valid artist candidate search".into()))?;
        let body = serde_json::to_value(request).map_err(|_| {
            Error::Internal("The artist candidate request could not be encoded".into())
        })?;
        let mut remaining = MAX_ARTIST_SEARCH_PAGE_BYTES;
        let page: ArtistSearchPage = self
            .exchange_admitted(
                base_url,
                token,
                Call {
                    path: ARTIST_SEARCH_PATH,
                    body: Some(body),
                    maximum: MAX_ARTIST_SEARCH_PAGE_BYTES,
                },
                &mut remaining,
            )
            .await?;
        page.validate_for(request)
            .map_err(|_| Error::Upstream("The provider returned invalid artist candidates"))?;
        Ok(page)
    }

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

    pub(in crate::plugins) async fn catalog_discography_supported_admitted(
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
                .contains(&CatalogOperation::ArtistDiscography))
    }

    pub(in crate::plugins) async fn catalog_discography_filter_supported_admitted(
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
                    path: CATALOG_DISCOGRAPHY_CAPABILITIES_PATH,
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
        let Ok(capabilities) = serde_json::from_value::<CatalogDiscographyCapabilities>(value)
        else {
            return Ok(false);
        };
        Ok(capabilities.validate().is_ok()
            && capabilities
                .operations
                .contains(&CatalogDiscographyOperation::ReleaseTypeFilter))
    }

    pub(in crate::plugins) async fn catalog_artist_discography_admitted(
        &self,
        base_url: &str,
        token: Option<&str>,
        request: &ArtistDiscographyRequest,
    ) -> Result<ArtistDiscographyPage> {
        request
            .validate()
            .map_err(|_| Error::Invalid("Use a valid artist discography request".into()))?;
        let body = serde_json::to_value(request).map_err(|_| {
            Error::Internal("The artist discography request could not be encoded".into())
        })?;
        let mut remaining = MAX_CATALOG_PAGE_BYTES;
        let page: ArtistDiscographyPage = self
            .exchange_admitted(
                base_url,
                token,
                Call {
                    path: ARTIST_DISCOGRAPHY_PATH,
                    body: Some(body),
                    maximum: MAX_CATALOG_PAGE_BYTES,
                },
                &mut remaining,
            )
            .await?;
        page.validate_for(request)
            .map_err(|_| Error::Upstream("The provider returned an invalid artist discography"))?;
        Ok(page)
    }
}
