use super::{COULD_NOT_REACH, Call, ControlClient, NOT_RESPONDING};
use crate::{Error, Result};
use hito_plugin_sdk::{HEALTH_PATH, HealthReport, MAX_HEALTH_BYTES};
use std::time::Duration;

const ANSWER_DEADLINE: Duration = Duration::from_secs(2);

impl ControlClient {
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub(in crate::plugins) async fn health(
        &self,
        base_url: &str,
        token: Option<&str>,
    ) -> Result<Option<HealthReport>> {
        let mut remaining = MAX_HEALTH_BYTES;
        let read = self.exchange_optional_admitted::<HealthReport>(
            base_url,
            token,
            Call {
                path: HEALTH_PATH,
                body: None,
                maximum: MAX_HEALTH_BYTES,
            },
            &mut remaining,
            true,
        );
        let report = tokio::time::timeout(ANSWER_DEADLINE, read)
            .await
            .map_err(|_| Error::Upstream(NOT_RESPONDING))??;
        report
            .map(|report| {
                report
                    .validate()
                    .map(|()| report)
                    .map_err(|_| Error::Upstream("The provider returned an invalid health report"))
            })
            .transpose()
    }
}

pub(in crate::plugins) fn is_unreachable(error: &Error) -> bool {
    matches!(error, Error::Upstream(message) if *message == NOT_RESPONDING || *message == COULD_NOT_REACH)
}
