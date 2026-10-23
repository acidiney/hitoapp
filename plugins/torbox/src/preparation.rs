use crate::{
    http::HttpState,
    journal::{Step, Target},
    policy::Error,
};
use hito_debrid_torbox::{Error as ProviderError, TorrentPhase};
use hito_plugin_sdk::{PreparationReason, PreparationStatus};

pub(crate) async fn settle(
    state: &HttpState,
    index: usize,
    step: Step,
) -> Result<(String, PreparationStatus), Error> {
    let status = match step.attempt {
        Some(mut attempt) => {
            let result = match state
                .client
                .verify_bound_file(&state.catalog.entry(index).binding)
                .await
            {
                Ok(verified) => {
                    attempt.mint_intent().await?;
                    verified.mint().await
                }
                Err(error) => Err(error),
            };
            attempt.finish(result).await?
        }
        None => step.status,
    };
    Ok((step.handle, status))
}

pub(crate) async fn ready(state: &HttpState, index: usize) -> Result<bool, Error> {
    let target = Target::from(state.catalog.entry(index));
    let Some(operation) = state.journal.candidate(target.clone()).await? else {
        return Ok(false);
    };
    match state
        .client
        .observe(&state.catalog.entry(index).binding)
        .await
    {
        Ok(current) if current.phase == TorrentPhase::ProviderCompleted => {
            Ok(state.journal.candidate(target.clone()).await? == Some(operation))
        }
        Ok(_) => {
            state
                .journal
                .invalidate(
                    target.clone(),
                    operation,
                    PreparationReason::SourceUnavailable,
                )
                .await?;
            Ok(false)
        }
        Err(ProviderError::SourceChanged) => {
            state
                .journal
                .invalidate(target.clone(), operation, PreparationReason::SourceChanged)
                .await?;
            Ok(false)
        }
        Err(_) => Err(Error::Unavailable),
    }
}
