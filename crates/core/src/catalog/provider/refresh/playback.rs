use super::{Binding, Catalog, Update};
use crate::{Actor, Error, Result, plugins::SourceBatch};
use futures_util::{StreamExt, stream::FuturesUnordered};
use hito_plugin_sdk::{Capability, RecordingRequest};
use std::time::Duration;
use tokio::time::Instant;

impl Catalog {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn refresh_playback_sources_until(
        &self,
        actor: &Actor,
        recording: &str,
        cancelled: impl std::future::Future<Output = ()> + Send,
    ) -> Result<bool> {
        let started = Instant::now();
        let deadline = started + Duration::from_secs(10);
        tokio::pin!(cancelled);
        let bindings = tokio::select! {
            biased;
            _ = &mut cancelled => return Ok(false),
            _ = tokio::time::sleep_until(deadline) => return Err(recovery_timeout()),
            result = self.refresh_bindings(actor, recording) => result?,
        };
        if bindings.is_empty() {
            return Ok(true);
        }
        let batch = tokio::select! {
            biased;
            _ = &mut cancelled => return Ok(false),
            _ = tokio::time::sleep_until(deadline) => return Err(recovery_timeout()),
            result = self.plugins.source_batch(actor, bindings.len()) => result?,
        };
        let mut bindings = bindings.into_iter();
        let mut pending = FuturesUnordered::new();
        for binding in bindings.by_ref().take(batch.parallelism()) {
            pending.push(self.prepare_playback_refresh(actor, binding, &batch));
        }
        let mut failure = None;
        while !pending.is_empty() {
            let prepared = tokio::select! {
                biased;
                _ = &mut cancelled => return Ok(false),
                _ = tokio::time::sleep_until(deadline) => return Err(recovery_timeout()),
                result = pending.next() => match result {
                    Some(result) => result,
                    None => break,
                },
            };
            match prepared {
                Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                Err(error) => {
                    if failure.is_none() {
                        failure = Some(error);
                    }
                }
                Ok(update) => {
                    tokio::select! {
                        biased;
                        _ = &mut cancelled => return Ok(false),
                        _ = tokio::time::sleep_until(deadline) => return Err(recovery_timeout()),
                        _ = std::future::ready(()) => {},
                    }
                    let ready = update.page.items.iter().any(|source| source.ready);
                    match self.publish_refresh(actor, recording, update).await {
                        Ok(()) if ready => {
                            tracing::info!(
                                elapsed_ms = started.elapsed().as_millis() as u64,
                                "Playback sources recovered"
                            );
                            return Ok(true);
                        }
                        Ok(()) => {}
                        Err(Error::Unauthorized) => return Err(Error::Unauthorized),
                        Err(error) => {
                            if failure.is_none() {
                                failure = Some(error);
                            }
                        }
                    }
                }
            }
            if let Some(binding) = bindings.next() {
                pending.push(self.prepare_playback_refresh(actor, binding, &batch));
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(true)
    }

    async fn prepare_playback_refresh(
        &self,
        actor: &Actor,
        binding: Binding,
        batch: &SourceBatch,
    ) -> Result<Update> {
        let operation = self
            .plugins
            .operation(
                actor,
                &binding.installation_id,
                binding.revision,
                &[Capability::Sources],
            )
            .await?;
        actor.check_session(&self.store).await?;
        let page = operation
            .sources_in_batch(
                batch,
                &RecordingRequest {
                    recording_id: binding.provider_recording_id.clone(),
                },
            )
            .await?;
        Ok(Update {
            binding,
            operation,
            page,
        })
    }
}

fn recovery_timeout() -> Error {
    Error::Upstream("Playback source recovery exceeded the time limit")
}
