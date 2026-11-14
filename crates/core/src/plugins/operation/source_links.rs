use super::Operation;
use crate::plugins::health::CredentialTicket;
use crate::{Error, Result};
use hito_plugin_sdk::{Capability, RecordingLookupRequest, RecordingMetadata};

impl Operation {
    pub(crate) async fn lookup_source_link(
        &self,
        request: &RecordingLookupRequest,
    ) -> Result<(Result<Option<RecordingMetadata>>, CredentialTicket)> {
        if !self.offers(Capability::SourceLinks) || !self.offers(Capability::Sources) {
            return Err(Error::Invalid(
                "This plugin has not been approved for source links".into(),
            ));
        }
        self.check_current().await?;
        let ticket = self.credential_observation();
        let result = self
            .remote
            .recording_lookup(
                &self.account,
                &self.base_url,
                self.token.as_deref().map(String::as_str),
                request,
            )
            .await;
        self.check_current().await?;
        Ok((result, ticket))
    }
}
