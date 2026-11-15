use super::{Call, ControlClient};
use crate::{Error, Result};
use hito_plugin_sdk::{
    MAX_RECORDING_METADATA_BYTES, RECORDING_LOOKUP_PATH, RecordingLookupRequest, RecordingMetadata,
};

const REJECTED: &str = "The plugin service rejected the request";

impl ControlClient {
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub(in crate::plugins) async fn recording_lookup(
        &self,
        account: &str,
        base_url: &str,
        token: Option<&str>,
        request: &RecordingLookupRequest,
    ) -> Result<Option<RecordingMetadata>> {
        request
            .validate()
            .map_err(|message| Error::Invalid(message.into()))?;
        let body = serde_json::to_value(request)
            .map_err(|_| Error::Internal("The source link lookup could not be encoded".into()))?;
        let _admission = self.admit(account)?;
        let mut remaining = MAX_RECORDING_METADATA_BYTES;
        let metadata: Option<RecordingMetadata> = self
            .exchange_optional_admitted(
                base_url,
                token,
                Call {
                    path: RECORDING_LOOKUP_PATH,
                    body: Some(body),
                    maximum: MAX_RECORDING_METADATA_BYTES,
                },
                &mut remaining,
                true,
            )
            .await
            .map_err(|error| match error {
                Error::UpstreamNotice(_) => Error::Upstream(REJECTED),
                other => other,
            })?;
        let Some(metadata) = metadata else {
            return Ok(None);
        };
        metadata
            .validate()
            .map_err(|_| Error::Upstream("The provider returned invalid recording metadata"))?;
        Ok(Some(metadata))
    }
}
