use super::{Call, ControlClient};
use crate::{Error, Result};
use hito_plugin_sdk::{FEATURED_PATH, FeaturedPage, FeaturedRequest, MAX_FEATURED_BYTES};

impl ControlClient {
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub(in crate::plugins) async fn featured(
        &self,
        account: &str,
        base_url: &str,
        token: Option<&str>,
        request: &FeaturedRequest,
    ) -> Result<FeaturedPage> {
        request
            .validate()
            .map_err(|_| Error::Invalid("Use a valid featured request".into()))?;
        let body = serde_json::to_value(request)
            .map_err(|_| Error::Internal("The featured request could not be encoded".into()))?;
        let page: FeaturedPage = self
            .exchange(
                account,
                base_url,
                token,
                Call {
                    path: FEATURED_PATH,
                    body: Some(body),
                    maximum: MAX_FEATURED_BYTES,
                },
            )
            .await?;
        page.validate_for(request)
            .map_err(|_| Error::Upstream("The provider returned invalid featured items"))?;
        Ok(page)
    }
}
