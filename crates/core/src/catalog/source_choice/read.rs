use super::{Catalog, SourceChoice, SourceChoiceMode, choice_access, recording_id};
use crate::{Actor, Result};
use std::time::Instant;

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.id()))]
    pub async fn source_choice(&self, actor: &Actor, recording: &str) -> Result<SourceChoice> {
        let started = Instant::now();
        let result = self.read_source_choice(actor, recording).await;
        tracing::debug!(
            event = "source_choice_read_finished",
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |_| "read"),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Source choice read finished"
        );
        result
    }

    async fn read_source_choice(&self, actor: &Actor, recording: &str) -> Result<SourceChoice> {
        let recording = recording_id(recording)?;
        let mut tx = self.store.pool.begin().await?;
        choice_access(&mut tx, actor, &recording).await?;
        let (revision, binding, reviewed): (i64, Option<String>, bool) = sqlx::query_as(
            "SELECT COALESCE(choice.revision,0),choice.binding_id,
                EXISTS(SELECT 1 FROM provider_items item WHERE item.recording_id=recording.id
                    AND item.retired=0 AND item.evidence='user_confirmed')
            FROM recordings recording LEFT JOIN recording_source_choices choice
                ON choice.recording_id=recording.id AND choice.user_id=?
            WHERE recording.id=?",
        )
        .bind(actor.id())
        .bind(&recording)
        .fetch_one(&mut *tx)
        .await?;
        let mode = match (binding.is_some(), revision > 0, reviewed) {
            (true, _, _) => SourceChoiceMode::Manual,
            (false, false, true) => SourceChoiceMode::Reviewed,
            _ => SourceChoiceMode::Automatic,
        };
        let sources = self.source_entries_on(&mut tx, actor, &recording).await?;
        tx.commit().await?;
        choice_access(&mut *self.store.pool.acquire().await?, actor, &recording).await?;
        Ok(SourceChoice {
            recording_id: recording,
            revision,
            mode,
            sources,
        })
    }
}
