use super::{Catalog, ProviderCandidate};
use crate::{Actor, Error, Result, identity::recording_access};
use hito_plugin_sdk::{Capability, RecordingLookupRequest};
use serde::Deserialize;
use sqlx::SqliteConnection;
use std::{future::Future, time::Duration};
use tokio::time::Instant;
use utoipa::ToSchema;

const LOOKUP_DEADLINE: Duration = Duration::from_secs(10);

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct LookupSourceLink {
    pub installation_id: String,
    pub installation_revision: i64,
    pub reference: String,
}

#[derive(PartialEq, sqlx::FromRow)]
struct Authority {
    library_id: String,
    metadata_binding_id: Option<String>,
    metadata_observation_id: Option<String>,
    choice_revision: i64,
}

async fn authority(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
) -> Result<Authority> {
    actor.check_session_on(connection).await?;
    recording_access(connection, actor, recording).await?;
    sqlx::query_as(
        "SELECT r.library_id,r.metadata_binding_id,r.metadata_observation_id,
            COALESCE((SELECT c.revision FROM recording_source_choices c WHERE c.user_id = ? AND c.recording_id = r.id),0) AS choice_revision
        FROM recordings r JOIN libraries l ON l.id = r.library_id AND l.owner_id = ? WHERE r.id = ?",
    )
    .bind(actor.id())
    .bind(actor.id())
    .bind(recording)
    .fetch_optional(connection)
    .await?
    .ok_or(Error::NotFound)
}

fn outcome(result: &Result<ProviderCandidate>) -> &'static str {
    match result {
        Ok(_) => "found",
        Err(Error::NotFound) => "not_found",
        Err(Error::Unauthorized | Error::Locked | Error::Forbidden) => "unauthorized",
        Err(Error::Conflict(_)) => "stale",
        Err(Error::Invalid(_)) => "invalid",
        Err(Error::ProviderBusy(_)) => "busy",
        Err(Error::Upstream(LOOKUP_CANCELLED)) => "cancelled",
        Err(Error::Upstream(LOOKUP_TIMEOUT)) => "deadline",
        Err(_) => "failed",
    }
}

const LOOKUP_CANCELLED: &str = "The source link lookup was cancelled";
const LOOKUP_TIMEOUT: &str =
    "The music service is not responding right now. Try again in a moment.";

impl Catalog {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn lookup_source_link(
        &self,
        actor: &Actor,
        recording: &str,
        request: LookupSourceLink,
        cancelled: impl Future<Output = ()> + Send,
    ) -> Result<ProviderCandidate> {
        let started = Instant::now();
        let deadline = started + LOOKUP_DEADLINE;
        tokio::pin!(cancelled);
        let result = tokio::select! {
            biased;
            _ = &mut cancelled => Err(Error::Upstream(LOOKUP_CANCELLED)),
            _ = tokio::time::sleep_until(deadline) => Err(Error::Upstream(LOOKUP_TIMEOUT)),
            result = self.lookup_source_link_owned(actor, recording, request) => result,
        };
        tracing::info!(
            event = "source_link_lookup_finished",
            outcome = outcome(&result),
            candidates = u64::from(result.is_ok()),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Source link lookup finished"
        );
        result
    }

    async fn lookup_source_link_owned(
        &self,
        actor: &Actor,
        recording: &str,
        request: LookupSourceLink,
    ) -> Result<ProviderCandidate> {
        let LookupSourceLink {
            installation_id,
            installation_revision,
            reference,
        } = request;
        let lookup = RecordingLookupRequest { reference };
        lookup
            .validate()
            .map_err(|message| Error::Invalid(message.into()))?;
        let before = authority(&mut *self.store.pool.acquire().await?, actor, recording).await?;
        let operation = self
            .plugins
            .operation(
                actor,
                &installation_id,
                installation_revision,
                &[Capability::Sources, Capability::SourceLinks],
            )
            .await?;
        let found = operation.lookup_source_link(&lookup).await;
        {
            let mut connection = self.store.pool.acquire().await?;
            let after = authority(&mut connection, actor, recording).await?;
            operation.recheck(&mut connection).await?;
            if after != before {
                return Err(Error::Conflict(
                    "The recording changed during the lookup. Paste the link again.".into(),
                ));
            }
        }
        let metadata = found?.ok_or(Error::NotFound)?;
        Ok(ProviderCandidate {
            installation_id,
            installation_revision: operation.catalog_installation_revision(),
            provider_recording_id: metadata.recording.id,
        })
    }
}
