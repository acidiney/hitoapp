use super::Operation;
use crate::Result;
use hito_plugin_sdk::HealthReport;

impl Operation {
    pub(in crate::plugins) fn connection_generation(
        &self,
    ) -> super::super::health::ConnectionGeneration {
        super::super::health::ConnectionGeneration {
            account: self.account.clone(),
            installation: self.installation.clone(),
            installation_revision: self.installation_revision,
            service: self.service.clone(),
            service_revision: self.service_revision,
        }
    }

    pub(crate) async fn health(&self) -> Result<Option<HealthReport>> {
        self.check_current().await?;
        let observation = self.credential_observation();
        let result = self
            .remote
            .health(&self.base_url, self.token.as_deref().map(String::as_str))
            .await;
        self.check_current().await?;
        result.map_err(|error| self.with_credential_context(error, &observation))
    }
}
