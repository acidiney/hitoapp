mod policy;

use super::Catalog;
use crate::{Actor, Error, Result, identity::recording_access};
pub(crate) use policy::push_source_choice;
use serde::{Deserialize, Serialize};
use sqlx::SqliteConnection;
use std::time::Instant;
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ChooseSource {
    pub request_id: Uuid,
    pub source_id: Uuid,
}

#[derive(Serialize, ToSchema)]
pub struct SourceChoiceReceipt {
    pub request_id: Uuid,
    pub recording_id: String,
    pub source_id: Uuid,
    pub revision: i64,
}

impl Catalog {
    #[tracing::instrument(level = "info", skip_all, fields(account_id = actor.id(), request_id = %request.request_id))]
    pub async fn choose_source(
        &self,
        actor: &Actor,
        recording: &str,
        request: ChooseSource,
    ) -> Result<SourceChoiceReceipt> {
        let started = Instant::now();
        let result = self.accept_source_choice(actor, recording, request).await;
        tracing::info!(
            event = "source_choice_finished",
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |_| "accepted"),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Source choice finished"
        );
        result
    }

    async fn accept_source_choice(
        &self,
        actor: &Actor,
        recording: &str,
        request: ChooseSource,
    ) -> Result<SourceChoiceReceipt> {
        if request.request_id.is_nil() || request.source_id.is_nil() {
            return Err(Error::Invalid(
                "Use nonempty source choice identities".into(),
            ));
        }
        let recording = Uuid::parse_str(recording)
            .ok()
            .filter(|id| !id.is_nil())
            .ok_or(Error::NotFound)?
            .to_string();
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        choice_access(&mut tx, actor, &recording).await?;
        let receipt: Option<(String, String, i64)> = sqlx::query_as(
            "SELECT recording_id,source_id,revision FROM source_choice_receipts WHERE user_id=? AND request_id=?",
        )
        .bind(actor.id())
        .bind(request.request_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if let Some((previous_recording, source, revision)) = receipt {
            if previous_recording != recording || source != request.source_id.to_string() {
                return Err(Error::Conflict(
                    "This source choice request was already used for another selection".into(),
                ));
            }
            tx.commit().await?;
            tracing::debug!(
                outcome = "replayed",
                revision,
                "Source choice acknowledgement replayed"
            );
            return Ok(SourceChoiceReceipt {
                request_id: request.request_id,
                recording_id: recording,
                source_id: request.source_id,
                revision,
            });
        }
        let binding: String = sqlx::query_scalar(
            "SELECT item.id FROM sources source
            JOIN provider_sources remote ON remote.source_id=source.id AND remote.recording_id=source.recording_id
            JOIN provider_items item ON item.id=remote.binding_id AND item.recording_id=remote.recording_id
                AND item.installation_id=remote.installation_id AND item.retired=0
            JOIN provider_source_state snapshot ON snapshot.binding_id=item.id AND snapshot.generation=remote.generation
            JOIN plugin_installations installation ON installation.id=item.installation_id AND installation.user_id=? AND installation.active=1
            JOIN plugin_services service ON service.id=installation.service_id AND service.active=1
            WHERE source.id=? AND source.recording_id=? AND source.provider='plugin'
                AND EXISTS(SELECT 1 FROM json_each(installation.manifest,'$.capabilities') WHERE value='sources')",
        )
        .bind(actor.id())
        .bind(request.source_id.to_string())
        .bind(&recording)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(Error::NotFound)?;
        let revision: i64 = sqlx::query_scalar(
            "INSERT INTO recording_source_choices(user_id,recording_id,revision,binding_id) VALUES(?,?,1,?)
            ON CONFLICT(user_id,recording_id) DO UPDATE SET revision=recording_source_choices.revision+1,binding_id=excluded.binding_id
            RETURNING revision",
        )
        .bind(actor.id())
        .bind(&recording)
        .bind(binding)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO source_choice_receipts(user_id,request_id,recording_id,source_id,revision) VALUES(?,?,?,?,?)")
            .bind(actor.id()).bind(request.request_id.to_string()).bind(&recording)
            .bind(request.source_id.to_string()).bind(revision).execute(&mut *tx).await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        Ok(SourceChoiceReceipt {
            request_id: request.request_id,
            recording_id: recording,
            source_id: request.source_id,
            revision,
        })
    }
}

async fn choice_access(
    connection: &mut SqliteConnection,
    actor: &Actor,
    recording: &str,
) -> Result<()> {
    actor.check_session_on(connection).await?;
    recording_access(connection, actor, recording).await?;
    let owned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM recordings recording JOIN libraries library ON library.id=recording.library_id WHERE recording.id=? AND library.owner_id=?)")
        .bind(recording).bind(actor.id()).fetch_one(connection).await?;
    if owned { Ok(()) } else { Err(Error::NotFound) }
}
