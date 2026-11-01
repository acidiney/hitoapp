use super::Operation;
use crate::Result;
use hito_plugin_sdk::{FeaturedPage, FeaturedRequest};

impl Operation {
    pub(crate) async fn featured(&self, request: &FeaturedRequest) -> Result<FeaturedPage> {
        self.check_current().await?;
        let result = self
            .remote
            .featured(
                &self.account,
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                request,
            )
            .await;
        self.check_current().await?;
        result
    }

    pub(crate) async fn check_current(&self) -> Result<()> {
        let mut connection = self.store.pool.acquire().await?;
        self.actor.check_session_on(&mut connection).await?;
        self.recheck(&mut connection).await
    }
}
