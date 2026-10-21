mod state;

use crate::{http::HttpState, policy::Error};
use hito_debrid_torbox::{Error as ProviderError, TorrentPhase};
use hito_plugin_sdk::{PreparationReason, PreparationStatus};
pub(crate) use state::Operations;

pub(crate) async fn settle(
    state: &HttpState,
    index: usize,
    step: state::Step,
) -> Result<(String, PreparationStatus), Error> {
    let status = match step.attempt {
        Some(attempt) => {
            let result = state
                .client
                .prepare_bound_file(&state.catalog.entry(index).binding)
                .await;
            attempt.finish(result)?
        }
        None => step.status,
    };
    Ok((step.handle, status))
}

pub(crate) async fn ready(state: &HttpState, index: usize) -> Result<bool, Error> {
    let Some(operation) = state.operations.candidate(index)? else {
        return Ok(false);
    };
    match state
        .client
        .observe(&state.catalog.entry(index).binding)
        .await
    {
        Ok(current) if current.phase == TorrentPhase::ProviderCompleted => {
            Ok(state.operations.candidate(index)? == Some(operation))
        }
        Ok(_) => {
            state
                .operations
                .invalidate(index, operation, PreparationReason::SourceUnavailable)?;
            Ok(false)
        }
        Err(ProviderError::SourceChanged) => {
            state
                .operations
                .invalidate(index, operation, PreparationReason::SourceChanged)?;
            Ok(false)
        }
        Err(_) => Err(Error::Unavailable),
    }
}
