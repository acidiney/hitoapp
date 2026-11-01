use super::genre_lookups::{self, GENRE_CURSOR};
use super::{
    ArtistMatchingState, ArtistMatchingStatus, MatchArtistsFromProviderEvidence,
    MatchArtistsFromTags, MetadataProvider, policy,
};
use crate::{Actor, ArtistCatalog, Error, Result};
use sqlx::{FromRow, SqliteConnection};
use std::time::Instant;
use uuid::Uuid;

const GROUP_CURSOR: &str = "groups:";
const LEASE_SECONDS: i64 = 90;
/// Walks that met an unavailable provider are repeated this many times, with the delay doubling
/// from two minutes up to an hour.
const MAX_ATTEMPTS: i64 = 8;
const BUSY_RETRY_SECONDS: i64 = 2;

#[derive(FromRow)]
struct Job {
    user_id: String,
    phase: String,
    cursor: Option<String>,
    rerun: bool,
    unavailable: bool,
    attempts: i64,
    matched: i64,
    lease_token: String,
}

enum Step {
    Continue {
        phase: &'static str,
        cursor: Option<String>,
        unavailable: bool,
        matched: i64,
    },
    Complete {
        unavailable: bool,
        matched: i64,
    },
    Dropped,
    Busy,
    Failed,
}

/// Schedules a background matching walk for an account whose settings allow it. Repeated
/// requests share the account's single job; a request that arrives while the walk is under way
/// asks for one more walk afterwards.
pub(crate) async fn request_for_account(
    connection: &mut SqliteConnection,
    account: &str,
) -> Result<()> {
    if !account_exists(connection, account).await? {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO artist_matching_jobs(user_id,next_run_at) VALUES(?,unixepoch())
         ON CONFLICT(user_id) DO UPDATE SET rerun = CASE
            WHEN (lease_token IS NOT NULL AND lease_until > unixepoch()) OR cursor IS NOT NULL OR phase <> 'tags'
            THEN 1 ELSE rerun END,
            next_run_at = MIN(next_run_at,unixepoch())",
    )
    .bind(account)
    .execute(connection)
    .await?;
    Ok(())
}

pub(crate) async fn request_for_library(
    connection: &mut SqliteConnection,
    library: &str,
) -> Result<()> {
    let members: Vec<String> =
        sqlx::query_scalar("SELECT user_id FROM library_members WHERE library_id = ?")
            .bind(library)
            .fetch_all(&mut *connection)
            .await?;
    for member in members {
        request_for_account(connection, &member).await?;
    }
    Ok(())
}

async fn lookups_allowed(connection: &mut SqliteConnection, account: &str) -> Result<bool> {
    let actor = match Actor::unattended_on(connection, account).await {
        Ok(actor) => actor,
        Err(Error::NotFound) => return Ok(false),
        Err(error) => return Err(error),
    };
    let settings = policy::read(connection, &actor).await?;
    Ok(settings.automatic_artist_lookup
        && policy::selected(&settings, &MetadataProvider::BuiltinMusicBrainz))
}

async fn account_exists(connection: &mut SqliteConnection, account: &str) -> Result<bool> {
    match Actor::unattended_on(connection, account).await {
        Ok(_) => Ok(true),
        Err(Error::NotFound) => Ok(false),
        Err(error) => Err(error),
    }
}

impl ArtistCatalog {
    /// Queues a matching walk for every account that can read a library, has automatic lookups
    /// on and either has never had a walk or names MusicBrainz identities whose genres were never
    /// read or are older than the refresh period. Accounts that already have a job are left alone.
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub async fn queue_first_automatic_matching(&self) -> Result<usize> {
        let started = Instant::now();
        let accounts: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT m.user_id FROM library_members m
             WHERE NOT EXISTS (SELECT 1 FROM artist_matching_jobs j WHERE j.user_id = m.user_id)",
        )
        .fetch_all(&self.store.pool)
        .await?;
        let mut queued = 0;
        for account in accounts {
            let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
            let job_started: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM artist_matching_jobs WHERE user_id = ?1)",
            )
            .bind(&account)
            .fetch_one(&mut *tx)
            .await?;
            let finished: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM account_metadata_automation
                    WHERE user_id = ?1 AND matching_finished_at IS NOT NULL)",
            )
            .bind(&account)
            .fetch_one(&mut *tx)
            .await?;
            if !job_started
                && (!finished || genre_lookups::has_pending(&mut tx, &account).await?)
                && lookups_allowed(&mut tx, &account).await?
            {
                request_for_account(&mut tx, &account).await?;
                let now_queued: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM artist_matching_jobs WHERE user_id = ?)",
                )
                .bind(&account)
                .fetch_one(&mut *tx)
                .await?;
                queued += usize::from(now_queued);
            }
            tx.commit().await?;
        }
        tracing::info!(
            event = "first_automatic_matching_queued",
            queued,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "First automatic artist matching walks queued"
        );
        Ok(queued)
    }

    /// Runs one batch of the oldest due matching walk: the tag pass, then the provider evidence
    /// and name passes, resuming from the stored position. A shutdown abandons the batch and
    /// leaves the walk to continue from the same position.
    #[tracing::instrument(level = "debug", skip_all, err(Debug))]
    pub async fn run_next_automatic_matching_until(
        &self,
        shutdown: impl std::future::Future<Output = ()> + Send,
    ) -> Result<bool> {
        let Some(job) = self.claim_matching_job().await? else {
            return Ok(false);
        };
        let started = Instant::now();
        let phase = job.phase.clone();
        let step = tokio::select! {
            biased;
            _ = shutdown => {
                self.release_matching_job(&job, 0).await?;
                tracing::info!(event = "automatic_matching_interrupted", phase,
                    elapsed_ms = started.elapsed().as_millis() as u64,
                    "Automatic artist matching step interrupted");
                return Ok(true);
            }
            step = self.matching_step(&job) => step,
        };
        let outcome = self.finish_matching_job(&job, step).await?;
        tracing::info!(
            event = "automatic_matching_step",
            phase,
            outcome,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Automatic artist matching step finished"
        );
        Ok(true)
    }

    async fn claim_matching_job(&self) -> Result<Option<Job>> {
        let due: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM artist_matching_jobs INDEXED BY artist_matching_jobs_due
             WHERE next_run_at <= unixepoch() AND (lease_token IS NULL OR lease_until <= unixepoch()))",
        )
        .fetch_one(&self.store.pool)
        .await?;
        if !due {
            return Ok(None);
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let token = Uuid::new_v4().to_string();
        let job: Option<Job> = sqlx::query_as(
            "UPDATE artist_matching_jobs SET lease_token = ?1,lease_until = unixepoch() + ?2
             WHERE user_id = (SELECT user_id FROM artist_matching_jobs
                WHERE next_run_at <= unixepoch() AND (lease_token IS NULL OR lease_until <= unixepoch())
                ORDER BY requested_at,user_id LIMIT 1)
             RETURNING user_id,phase,cursor,rerun,unavailable,attempts,matched,lease_token",
        )
        .bind(&token)
        .bind(LEASE_SECONDS)
        .fetch_optional(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(job)
    }

    async fn matching_step(&self, job: &Job) -> Step {
        let actor = async {
            let mut connection = self.store.pool.acquire().await?;
            let actor = Actor::unattended_on(&mut connection, &job.user_id).await?;
            let settings = policy::read(&mut connection, &actor).await?;
            let allowed = settings.automatic_artist_lookup
                && policy::selected(&settings, &MetadataProvider::BuiltinMusicBrainz);
            Ok::<_, Error>(allowed.then_some(actor))
        }
        .await;
        let actor = match actor {
            Ok(Some(actor)) => actor,
            Ok(None) => return self.grouping_only(job).await,
            Err(Error::NotFound) => return Step::Dropped,
            Err(_) => return Step::Failed,
        };
        let request_id = Uuid::new_v4();
        let genre_position = job
            .cursor
            .as_deref()
            .and_then(|cursor| cursor.strip_prefix(GENRE_CURSOR))
            .filter(|_| job.phase == "provider");
        let group_position = job
            .cursor
            .as_deref()
            .and_then(|cursor| cursor.strip_prefix(GROUP_CURSOR))
            .filter(|_| job.phase == "provider");
        let result = if let Some(position) = group_position {
            crate::catalog::group_step(
                &self.store,
                &job.user_id,
                Some(position).filter(|p| !p.is_empty()),
            )
            .await
            .map(|step| (step.next, false, "groups", step.grouped))
        } else if let Some(position) = genre_position {
            self.collect_musicbrainz_genres(&actor, Some(position).filter(|p| !p.is_empty()))
                .await
                .map(|receipt| (receipt.next_after, receipt.unavailable > 0, "genres", 0))
        } else if job.phase == "tags" {
            self.match_artists_from_tags(
                &actor,
                MatchArtistsFromTags {
                    request_id,
                    after: job.cursor.clone(),
                    limit: None,
                },
            )
            .await
            .map(|receipt| (receipt.next_after, false, "tags", receipt.matched))
        } else {
            self.match_artists_from_provider_evidence(
                &actor,
                MatchArtistsFromProviderEvidence {
                    request_id,
                    after: job.cursor.clone(),
                    limit: None,
                },
            )
            .await
            .map(|receipt| {
                (
                    receipt.next_after,
                    receipt.unavailable > 0,
                    "provider",
                    receipt.matched,
                )
            })
        };
        match result {
            Ok((Some(cursor), unavailable, "genres", matched)) => Step::Continue {
                phase: "provider",
                cursor: Some(format!("{GENRE_CURSOR}{cursor}")),
                unavailable,
                matched,
            },
            Ok((None, unavailable, "genres", matched)) => Step::Continue {
                phase: "provider",
                cursor: Some(GROUP_CURSOR.to_owned()),
                unavailable,
                matched,
            },
            Ok((Some(cursor), unavailable, "groups", _)) => Step::Continue {
                phase: "provider",
                cursor: Some(format!("{GROUP_CURSOR}{cursor}")),
                unavailable,
                matched: 0,
            },
            Ok((None, unavailable, "groups", _)) => Step::Complete {
                unavailable,
                matched: 0,
            },
            Ok((Some(cursor), unavailable, phase, matched)) => Step::Continue {
                phase,
                cursor: Some(cursor),
                unavailable,
                matched,
            },
            Ok((None, unavailable, "tags", matched)) => Step::Continue {
                phase: "provider",
                cursor: None,
                unavailable,
                matched,
            },
            Ok((None, unavailable, _, matched)) => Step::Continue {
                phase: "provider",
                cursor: Some(GENRE_CURSOR.to_owned()),
                unavailable,
                matched,
            },
            Err(Error::Conflict(_)) => Step::Busy,
            Err(Error::Unauthorized | Error::NotFound | Error::Forbidden) => Step::Dropped,
            Err(_) => Step::Failed,
        }
    }

    async fn grouping_only(&self, job: &Job) -> Step {
        let position = job
            .cursor
            .as_deref()
            .filter(|_| job.phase == "provider")
            .and_then(|cursor| cursor.strip_prefix(GROUP_CURSOR));
        let Some(position) = position else {
            return Step::Continue {
                phase: "provider",
                cursor: Some(GROUP_CURSOR.to_owned()),
                unavailable: false,
                matched: 0,
            };
        };
        let position = Some(position).filter(|p| !p.is_empty());
        match crate::catalog::group_step(&self.store, &job.user_id, position).await {
            Ok(step) => match step.next {
                Some(next) => Step::Continue {
                    phase: "provider",
                    cursor: Some(format!("{GROUP_CURSOR}{next}")),
                    unavailable: false,
                    matched: 0,
                },
                None => Step::Dropped,
            },
            Err(Error::Conflict(_)) => Step::Busy,
            Err(_) => Step::Failed,
        }
    }

    async fn finish_matching_job(&self, job: &Job, step: Step) -> Result<&'static str> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let outcome = match step {
            Step::Continue {
                phase,
                cursor,
                unavailable,
                matched,
            } => {
                sqlx::query(
                    "UPDATE artist_matching_jobs SET phase = ?,cursor = ?,matched = matched + ?,
                    unavailable = unavailable OR ?,lease_token = NULL,lease_until = NULL,
                    next_run_at = unixepoch() WHERE user_id = ? AND lease_token = ?",
                )
                .bind(phase)
                .bind(cursor)
                .bind(matched)
                .bind(unavailable)
                .bind(&job.user_id)
                .bind(&job.lease_token)
                .execute(&mut *tx)
                .await?;
                "advanced"
            }
            Step::Complete {
                unavailable,
                matched,
            } => {
                sqlx::query("INSERT INTO account_metadata_automation(user_id,matching_finished_at,matching_matched)
                    VALUES (?,unixepoch(),?) ON CONFLICT(user_id) DO UPDATE SET
                    matching_finished_at = excluded.matching_finished_at,matching_matched = excluded.matching_matched")
                    .bind(&job.user_id)
                    .bind(job.matched + matched)
                    .execute(&mut *tx)
                    .await?;
                sqlx::query(
                    "INSERT OR IGNORE INTO cover_art_lookups(user_id,kind,mbid,state)
                     SELECT DISTINCT ?1,o.entity,o.mbid,'queued'
                     FROM recording_musicbrainz_observations o
                     JOIN recordings r ON r.id = o.recording_id AND r.metadata_observation_id = o.observation_id
                     JOIN releases a ON a.id = o.release_id
                     JOIN library_members m ON m.library_id = a.library_id AND m.user_id = ?1
                     WHERE o.entity IN ('release','release_group')
                        AND NOT EXISTS (SELECT 1 FROM release_artwork ra WHERE ra.user_id = ?1 AND ra.release_id = a.id)
                        AND NOT EXISTS (SELECT 1 FROM source_metadata sm JOIN sources s ON s.id = sm.source_id
                            WHERE sm.release_id = a.id AND s.provider = 'local')
                        AND NOT EXISTS (SELECT 1 FROM cover_art_lookups c
                            WHERE c.user_id = ?1 AND c.kind = o.entity AND c.mbid = o.mbid)
                        AND COALESCE((SELECT artwork FROM account_metadata_automation WHERE user_id = ?1),1) = 1
                     LIMIT 32",
                )
                .bind(&job.user_id)
                .execute(&mut *tx)
                .await?;
                let unavailable = job.unavailable || unavailable;
                let attempts = job.attempts + i64::from(unavailable);
                if unavailable && attempts >= MAX_ATTEMPTS {
                    tracing::info!(
                        event = "automatic_matching_walk_abandoned",
                        attempt = attempts,
                        max_attempts = MAX_ATTEMPTS,
                        "Automatic artist matching walk abandoned after repeated unavailable lookups"
                    );
                    delete(&mut tx, job).await?;
                    "abandoned"
                } else if job.rerun || unavailable {
                    let delay = if unavailable {
                        (60i64 << attempts.min(6)).min(3600)
                    } else {
                        0
                    };
                    sqlx::query("UPDATE artist_matching_jobs SET phase = 'tags',cursor = NULL,rerun = 0,unavailable = 0,matched = 0,
                        attempts = ?,lease_token = NULL,lease_until = NULL,next_run_at = unixepoch() + ?
                        WHERE user_id = ? AND lease_token = ?")
                        .bind(if unavailable { attempts } else { 0 })
                        .bind(delay)
                        .bind(&job.user_id)
                        .bind(&job.lease_token)
                        .execute(&mut *tx)
                        .await?;
                    if unavailable {
                        tracing::info!(
                            event = "automatic_matching_walk_rescheduled",
                            attempt = attempts,
                            max_attempts = MAX_ATTEMPTS,
                            delay_seconds = delay,
                            matched = job.matched + matched,
                            "Automatic artist matching walk rescheduled after unavailable lookups"
                        );
                        "retry_scheduled"
                    } else {
                        "rescheduled"
                    }
                } else {
                    delete(&mut tx, job).await?;
                    "complete"
                }
            }
            Step::Dropped => {
                delete(&mut tx, job).await?;
                "dropped"
            }
            Step::Busy => {
                release(&mut tx, job, BUSY_RETRY_SECONDS).await?;
                "busy"
            }
            Step::Failed => {
                if job.attempts + 1 >= MAX_ATTEMPTS {
                    delete(&mut tx, job).await?;
                    "abandoned"
                } else {
                    sqlx::query("UPDATE artist_matching_jobs SET attempts = attempts + 1,lease_token = NULL,
                        lease_until = NULL,next_run_at = unixepoch() + ? WHERE user_id = ? AND lease_token = ?")
                        .bind((60i64 << job.attempts.min(6)).min(3600))
                        .bind(&job.user_id)
                        .bind(&job.lease_token)
                        .execute(&mut *tx)
                        .await?;
                    "failed"
                }
            }
        };
        tx.commit().await?;
        Ok(outcome)
    }

    async fn release_matching_job(&self, job: &Job, delay: i64) -> Result<()> {
        let mut connection = self.store.pool.acquire().await?;
        release(&mut connection, job, delay).await
    }
}

async fn delete(connection: &mut SqliteConnection, job: &Job) -> Result<()> {
    sqlx::query("DELETE FROM artist_matching_jobs WHERE user_id = ? AND lease_token = ?")
        .bind(&job.user_id)
        .bind(&job.lease_token)
        .execute(connection)
        .await?;
    Ok(())
}

async fn release(connection: &mut SqliteConnection, job: &Job, delay: i64) -> Result<()> {
    sqlx::query(
        "UPDATE artist_matching_jobs SET lease_token = NULL,lease_until = NULL,
        next_run_at = unixepoch() + ? WHERE user_id = ? AND lease_token = ?",
    )
    .bind(delay)
    .bind(&job.user_id)
    .bind(&job.lease_token)
    .execute(connection)
    .await?;
    Ok(())
}

impl ArtistCatalog {
    #[tracing::instrument(
        level = "debug",
        skip_all,
        fields(operation = "artist_matching_status")
    )]
    pub async fn artist_matching_status(&self, actor: &Actor) -> Result<ArtistMatchingStatus> {
        let mut connection = self.store.pool.acquire().await?;
        actor.check_session_on(&mut connection).await?;
        let settings = policy::read(&mut connection, actor).await?;
        let enabled = settings.automatic_artist_lookup
            && policy::selected(&settings, &MetadataProvider::BuiltinMusicBrainz);
        let job: Option<(i64, bool)> = sqlx::query_as(
            "SELECT matched,lease_token IS NOT NULL AND lease_until > unixepoch() FROM artist_matching_jobs WHERE user_id = ?",
        )
        .bind(actor.id())
        .fetch_optional(&mut *connection)
        .await?;
        let last: Option<(Option<i64>, i64)> = sqlx::query_as(
            "SELECT matching_finished_at,matching_matched FROM account_metadata_automation WHERE user_id = ?",
        )
        .bind(actor.id())
        .fetch_optional(&mut *connection)
        .await?;
        let (last_finished_at, last_matched) = last.unwrap_or((None, 0));
        let (state, matched_so_far) = match job {
            _ if !enabled => (ArtistMatchingState::Idle, 0),
            Some((matched, true)) => (ArtistMatchingState::Running, matched),
            Some((matched, false)) => (ArtistMatchingState::Queued, matched),
            None => (ArtistMatchingState::Idle, 0),
        };
        Ok(ArtistMatchingStatus {
            enabled,
            state,
            matched_so_far,
            last_finished_at,
            last_matched,
        })
    }
}
