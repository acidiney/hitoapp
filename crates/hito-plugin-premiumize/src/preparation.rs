use crate::{CuratedWavRecording, config::Connection, policy::Error};
use hito_debrid_premiumize::{Error as ProviderError, TransferPhase};
use hito_plugin_sdk::{PreparationReason, PreparationStatus};

pub(crate) async fn ready(
    connection: &Connection,
    entry: &CuratedWavRecording,
) -> Result<bool, Error> {
    match connection.client.observe_bound_file(&entry.binding).await {
        Ok(_) => Ok(true),
        Err(
            ProviderError::UnsupportedState
            | ProviderError::TransferNotFound
            | ProviderError::TransferChanged
            | ProviderError::FileChanged,
        ) => Ok(false),
        Err(_) => Err(Error::Unavailable),
    }
}

pub(crate) async fn status(
    connection: &Connection,
    entry: &CuratedWavRecording,
) -> Result<PreparationStatus, Error> {
    let observed = connection
        .client
        .observe(&entry.binding.transfer)
        .await
        .map_err(|_| Error::Unavailable)?;
    match observed.phase {
        TransferPhase::Queued => Ok(PreparationStatus::Queued {
            poll_after_seconds: 5,
        }),
        TransferPhase::Running => Ok(PreparationStatus::Preparing {
            progress_basis_points: Some((observed.progress.fraction() * 10_000.0).round() as u16),
            eta_seconds: None,
            poll_after_seconds: 5,
        }),
        TransferPhase::ProviderFailed => Ok(PreparationStatus::Failed {
            reason: PreparationReason::SourceUnavailable,
        }),
        TransferPhase::ProviderSeeding | TransferPhase::ProviderCompleted => {
            if ready(connection, entry).await? {
                Ok(PreparationStatus::Ready {})
            } else {
                Ok(PreparationStatus::Failed {
                    reason: PreparationReason::SourceUnavailable,
                })
            }
        }
    }
}
