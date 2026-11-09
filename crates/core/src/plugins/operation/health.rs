use super::Operation;
use crate::Result;
use hito_plugin_sdk::HealthReport;

impl Operation {
    pub(crate) async fn health(&self) -> Result<Option<HealthReport>> {
        self.remote
            .health(&self.base_url, self.token.as_deref().map(String::as_str))
            .await
    }
}
