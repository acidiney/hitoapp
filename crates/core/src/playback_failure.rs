use crate::Error;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackFailureReason {
    ProviderUnavailable,
    ProviderRefused,
    CredentialsRequired,
    InvalidProviderResponse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PlaybackFailure {
    pub reason: PlaybackFailureReason,
    pub retryable: bool,
}

impl PlaybackFailure {
    pub(crate) fn new(reason: PlaybackFailureReason) -> Self {
        Self {
            reason,
            retryable: reason == PlaybackFailureReason::ProviderUnavailable,
        }
    }

    pub(crate) fn from_provider_error(error: &Error) -> Option<Self> {
        let reason = match error {
            Error::PlaybackProvider { reason, .. } => *reason,
            Error::ProviderBusy(_) => PlaybackFailureReason::ProviderUnavailable,
            Error::PluginCredentialsRequired(_) | Error::AuthorizationRequired(_) => {
                PlaybackFailureReason::CredentialsRequired
            }
            Error::UpstreamNotice(_) | Error::OperationRejected(_) => {
                PlaybackFailureReason::ProviderRefused
            }
            _ => return None,
        };
        Some(Self::new(reason))
    }

    pub(crate) fn merge(current: &mut Option<Self>, incoming: Option<Self>) {
        if let Some(incoming) = incoming
            && current.is_none_or(|current| incoming.retryable && !current.retryable)
        {
            *current = Some(incoming);
        }
    }

    pub(crate) fn valid(self) -> bool {
        self == Self::new(self.reason)
    }
}
