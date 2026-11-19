use crate::{Actor, Catalog, Error, Result};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::{future::Future, sync::atomic::Ordering, time::Instant};
use utoipa::ToSchema;
use uuid::Uuid;

const LEASE_SECONDS: i64 = 90;
const PACE_SECONDS: i64 = 120;
const PLAYING_SECONDS: i64 = 120;
const STALE_SECONDS: i64 = 3 * 24 * 60 * 60;
const WALK_AGAIN_SECONDS: i64 = 6 * 60 * 60;
const DISABLED_RECHECK_SECONDS: i64 = 60 * 60;
const BUSY_RETRY_SECONDS: i64 = 60;
const CHEAP_CHECKS_PER_STEP: usize = 50;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceReadinessPreference {
    pub enabled: bool,
}

#[derive(FromRow)]
struct Job {
    user_id: String,
    cursor: Option<String>,
    attempts: i64,
    lease_token: String,
}

enum Step {
    Paced {
        cursor: Option<String>,
    },
    Waiting {
        cursor: Option<String>,
        seconds: i64,
    },
    Walked,
    Dropped,
    Failed {
        cursor: Option<String>,
    },
}

impl Step {
    fn name(&self) -> &'static str {
        match self {
            Self::Paced { .. } => "paced",
            Self::Waiting { .. } => "waiting",
            Self::Walked => "walked",
            Self::Dropped => "dropped",
            Self::Failed { .. } => "failed",
        }
    }
}

impl Catalog {
    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn source_readiness_preference(
        &self,
        actor: &Actor,
    ) -> Result<SourceReadinessPreference> {
        actor.check_session(&self.store).await?;
        let enabled: Option<bool> = sqlx::query_scalar(
            "SELECT enabled FROM account_source_readiness_preferences WHERE user_id = ?",
        )
        .bind(actor.id())
        .fetch_optional(&self.store.pool)
        .await?;
        Ok(SourceReadinessPreference {
            enabled: enabled.unwrap_or(true),
        })
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), enabled = preference.enabled))]
    pub async fn set_source_readiness_preference(
        &self,
        actor: &Actor,
        preference: SourceReadinessPreference,
    ) -> Result<SourceReadinessPreference> {
        actor.require_unlocked()?;
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        sqlx::query(
            "INSERT INTO account_source_readiness_preferences(user_id,enabled) VALUES(?1,?2)
             ON CONFLICT(user_id) DO UPDATE SET enabled = excluded.enabled",
        )
        .bind(actor.id())
        .bind(preference.enabled)
        .execute(&mut *tx)
        .await?;
        if preference.enabled {
            sqlx::query(
                "INSERT INTO source_readiness_jobs(user_id,next_run_at) VALUES(?1,unixepoch())
                 ON CONFLICT(user_id) DO UPDATE SET next_run_at = MIN(next_run_at,unixepoch())",
            )
            .bind(actor.id())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        tracing::info!(
            event = "source_readiness_preference_changed",
            "Background source preparation setting saved"
        );
        Ok(preference)
    }

    pub fn hold_background_preparation(&self, held: bool) {
        let was = self.preparation_held.swap(held, Ordering::AcqRel);
        if was != held {
            tracing::info!(
                event = "source_readiness_hold_changed",
                held,
                "Background source preparation hold changed"
            );
        }
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub async fn run_next_source_readiness_until(
        &self,
        shutdown: impl Future<Output = ()> + Send,
    ) -> Result<bool> {
        if self.preparation_held.load(Ordering::Acquire) {
            return Ok(false);
        }
        let Some(job) = self.claim_readiness_job().await? else {
            return Ok(false);
        };
        let started = Instant::now();
        tokio::pin!(shutdown);
        let step = self.readiness_step(&job, shutdown.as_mut()).await;
        let outcome = step.name();
        self.finish_readiness_job(&job, step).await?;
        tracing::debug!(
            event = "source_readiness_step",
            outcome,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Background source preparation step finished"
        );
        Ok(true)
    }

    async fn claim_readiness_job(&self) -> Result<Option<Job>> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let queued = sqlx::query(
            "INSERT OR IGNORE INTO source_readiness_jobs(user_id,next_run_at)
             SELECT DISTINCT user_id,unixepoch() + ?1 FROM library_members",
        )
        .bind(PACE_SECONDS)
        .execute(&mut *tx)
        .await?
        .rows_affected();
        if queued > 0 {
            tracing::debug!(
                event = "source_readiness_queued",
                queued,
                "Background source preparation walks queued"
            );
        }
        let job: Option<Job> = sqlx::query_as(
            "UPDATE source_readiness_jobs SET lease_token = ?1,lease_until = unixepoch() + ?2
             WHERE user_id = (SELECT user_id FROM source_readiness_jobs INDEXED BY source_readiness_jobs_due
                WHERE next_run_at <= unixepoch() AND (lease_token IS NULL OR lease_until <= unixepoch())
                ORDER BY next_run_at,user_id LIMIT 1)
             RETURNING user_id,cursor,attempts,lease_token",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(LEASE_SECONDS)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(job)
    }

    async fn readiness_step(
        &self,
        job: &Job,
        mut shutdown: std::pin::Pin<&mut (impl Future<Output = ()> + Send)>,
    ) -> Step {
        let cursor = job.cursor.clone();
        let outcome = async {
            let mut connection = self.store.pool.acquire().await?;
            let actor = match Actor::unattended_on(&mut connection, &job.user_id).await {
                Ok(actor) => actor,
                Err(Error::NotFound) => return Ok(Step::Dropped),
                Err(error) => return Err(error),
            };
            let enabled: Option<bool> = sqlx::query_scalar(
                "SELECT enabled FROM account_source_readiness_preferences WHERE user_id = ?",
            )
            .bind(&job.user_id)
            .fetch_optional(&mut *connection)
            .await?;
            if !enabled.unwrap_or(true) {
                return Ok(Step::Waiting {
                    cursor: cursor.clone(),
                    seconds: DISABLED_RECHECK_SECONDS,
                });
            }
            let playing: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM listening_progress
                 WHERE user_id = ?1 AND updated_at > unixepoch() - ?2)",
            )
            .bind(&job.user_id)
            .bind(PLAYING_SECONDS)
            .fetch_one(&mut *connection)
            .await?;
            if playing {
                return Ok(Step::Waiting {
                    cursor: cursor.clone(),
                    seconds: BUSY_RETRY_SECONDS,
                });
            }
            let mut position = cursor.clone();
            for _ in 0..CHEAP_CHECKS_PER_STEP {
                let next: Option<String> = sqlx::query_scalar(
                    "SELECT r.id FROM recordings r
                     JOIN library_members m ON m.library_id = r.library_id AND m.user_id = ?1
                     WHERE r.id > COALESCE(?2,'')
                        AND NOT EXISTS (SELECT 1 FROM podcast_episodes p WHERE p.recording_id = r.id)
                        AND NOT EXISTS (SELECT 1 FROM source_readiness_checks c
                            WHERE c.user_id = ?1 AND c.recording_id = r.id AND c.checked_at > unixepoch() - ?3)
                     ORDER BY r.id LIMIT 1",
                )
                .bind(&job.user_id)
                .bind(&position)
                .bind(STALE_SECONDS)
                .fetch_optional(&mut *connection)
                .await?;
                let Some(recording) = next else {
                    return Ok(Step::Walked);
                };
                drop(connection);
                let sources = self.sources(&actor, &recording).await?;
                let provider_ready = sources
                    .iter()
                    .any(|source| source.ready && !local(&source.provider));
                let local_ready = sources
                    .iter()
                    .any(|source| source.ready && local(&source.provider));
                if local_ready && !provider_ready {
                    self.checked(&job.user_id, &recording).await?;
                    position = Some(recording);
                    connection = self.store.pool.acquire().await?;
                    continue;
                }
                if provider_ready {
                    let refreshed = self
                        .refresh_playback_sources_until(&actor, &recording, shutdown.as_mut())
                        .await;
                    if matches!(refreshed, Ok(false)) {
                        return Ok(Step::Waiting {
                            cursor: position,
                            seconds: 0,
                        });
                    }
                    self.checked(&job.user_id, &recording).await?;
                    refreshed?;
                    return Ok(Step::Paced {
                        cursor: Some(recording),
                    });
                }
                match self
                    .resolve_confident_source_in_background(&actor, &recording, shutdown.as_mut())
                    .await?
                {
                    None => {
                        return Ok(Step::Waiting {
                            cursor: position,
                            seconds: BUSY_RETRY_SECONDS,
                        });
                    }
                    Some(resolution) => {
                        self.checked(&job.user_id, &recording).await?;
                        tracing::debug!(
                            event = "source_readiness_resolved",
                            outcome = resolution.outcome.as_str(),
                            "Background source preparation looked for audio"
                        );
                        return Ok(Step::Paced {
                            cursor: Some(recording),
                        });
                    }
                }
            }
            Ok(Step::Waiting {
                cursor: position,
                seconds: 0,
            })
        }
        .await;
        match outcome {
            Ok(step) => step,
            Err(error) => {
                tracing::warn!(
                    event = "source_readiness_failed",
                    error_kind = error.code(),
                    "Background source preparation step failed"
                );
                Step::Failed { cursor }
            }
        }
    }

    async fn checked(&self, account: &str, recording: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO source_readiness_checks(user_id,recording_id,checked_at) VALUES(?1,?2,unixepoch())
             ON CONFLICT(user_id,recording_id) DO UPDATE SET checked_at = excluded.checked_at",
        )
        .bind(account)
        .bind(recording)
        .execute(&self.store.pool)
        .await?;
        Ok(())
    }

    async fn finish_readiness_job(&self, job: &Job, step: Step) -> Result<()> {
        let (cursor, delay, attempts) = match step {
            Step::Dropped => {
                sqlx::query(
                    "DELETE FROM source_readiness_jobs WHERE user_id = ?1 AND lease_token = ?2",
                )
                .bind(&job.user_id)
                .bind(&job.lease_token)
                .execute(&self.store.pool)
                .await?;
                return Ok(());
            }
            Step::Paced { cursor } => (cursor, PACE_SECONDS, 0),
            Step::Waiting { cursor, seconds } => (cursor, seconds, job.attempts),
            Step::Walked => (None, WALK_AGAIN_SECONDS, 0),
            Step::Failed { cursor } => {
                let attempts = job.attempts + 1;
                (cursor, (60_i64 << attempts.min(6)).min(3600), attempts)
            }
        };
        sqlx::query(
            "UPDATE source_readiness_jobs SET cursor = ?3,attempts = ?4,next_run_at = unixepoch() + ?5,
                lease_token = NULL,lease_until = NULL
             WHERE user_id = ?1 AND lease_token = ?2",
        )
        .bind(&job.user_id)
        .bind(&job.lease_token)
        .bind(cursor)
        .bind(attempts)
        .bind(delay)
        .execute(&self.store.pool)
        .await?;
        Ok(())
    }
}

fn local(provider: &str) -> bool {
    matches!(provider, "local" | "download")
}
