use super::{Collection, RecordingProgress};
use crate::{Actor, Error, Result, store::Store};
use sqlx::QueryBuilder;
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};
use uuid::Uuid;

impl Collection {
    #[tracing::instrument(level = "debug", skip_all, fields(account_id = actor.id(), recording_count = recording_ids.len(), read_id = %Uuid::new_v4()))]
    pub async fn saved_playback(
        &self,
        actor: &Actor,
        recording_ids: &[Uuid],
    ) -> Result<Vec<RecordingProgress>> {
        let started = Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            saved_progress(&self.store, actor, recording_ids),
        )
        .await
        .unwrap_or(Err(Error::Upstream(
            "Saved playback could not be checked in time",
        )));
        match &result {
            Ok(saved) => tracing::debug!(
                saved_count = saved.len(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Saved playback loaded"
            ),
            Err(error) => tracing::debug!(
                error_code = error.code(),
                elapsed_ms = started.elapsed().as_millis() as u64,
                "Saved playback rejected"
            ),
        }
        result
    }
}

pub(crate) async fn saved_progress(
    store: &Store,
    actor: &Actor,
    recording_ids: &[Uuid],
) -> Result<Vec<RecordingProgress>> {
    if recording_ids.len() > 200 {
        return Err(Error::Invalid("Request at most 200 recordings".into()));
    }
    actor.check_session(store).await?;
    let mut seen = HashSet::with_capacity(recording_ids.len());
    let requested: Vec<Uuid> = recording_ids
        .iter()
        .copied()
        .filter(|recording_id| seen.insert(*recording_id))
        .collect();
    if requested.is_empty() {
        return Ok(Vec::new());
    }
    let mut query = QueryBuilder::new("WITH requested(recording_id, ordinal) AS (");
    query.push_values(requested.iter().enumerate(), |mut row, (ordinal, id)| {
        row.push_bind(id.to_string()).push_bind(ordinal as i64);
    });
    query.push(
        ") SELECT p.position_ms, p.speed FROM requested
         CROSS JOIN recordings r INDEXED BY sqlite_autoindex_recordings_1
            ON r.id = requested.recording_id
         CROSS JOIN library_members m INDEXED BY sqlite_autoindex_library_members_1
            ON m.library_id = r.library_id AND m.user_id = ",
    );
    query.push_bind(actor.id()).push(
        " LEFT JOIN listening_progress p INDEXED BY sqlite_autoindex_listening_progress_1
            ON p.recording_id = r.id AND p.user_id = m.user_id
          ORDER BY requested.ordinal",
    );
    let rows: Vec<(Option<i64>, Option<f64>)> =
        query.build_query_as().fetch_all(&store.pool).await?;
    actor.check_session(store).await?;
    if rows.len() != requested.len() {
        return Err(Error::NotFound);
    }
    Ok(rows
        .into_iter()
        .zip(requested)
        .filter_map(|((position_ms, speed), recording_id)| {
            position_ms
                .zip(speed)
                .map(|(position_ms, speed)| RecordingProgress {
                    recording_id,
                    position_ms: position_ms as u64,
                    speed,
                })
        })
        .collect())
}
