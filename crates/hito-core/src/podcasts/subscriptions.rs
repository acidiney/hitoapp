use super::{
    PodcastRefreshReceipt, PodcastSubscription, PodcastSubscriptionReceipt, Podcasts,
    RefreshPodcast, SubscribePodcast,
    lifecycle::{reactivate, subscription_state},
};
use crate::{Actor, Error, Result};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(sqlx::FromRow)]
struct SubscriptionRow {
    id: String,
    title: Option<String>,
    refresh_pending: bool,
    refresh_failed: bool,
    last_refreshed_at: Option<i64>,
    content_revision: i64,
}

impl Podcasts {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), request_id = %request.request_id))]
    pub async fn subscribe(
        &self,
        actor: &Actor,
        request: SubscribePodcast,
    ) -> Result<PodcastSubscriptionReceipt> {
        let started = std::time::Instant::now();
        let url = crate::remote_http::parse_url(&request.feed_url)?.to_string();
        let digest = format!("{:x}", Sha256::digest(url.as_bytes()));
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let replay: Option<(String, String, String)> = sqlx::query_as(
            "SELECT request_digest,subscription_id,job_id FROM podcast_subscription_receipts WHERE user_id=? AND request_id=?",
        ).bind(actor.id()).bind(request.request_id.to_string()).fetch_optional(&mut *tx).await?;
        if let Some((previous, subscription, job)) = replay {
            if previous != digest {
                return Err(Error::Conflict(
                    "This request was already used for a different podcast".into(),
                ));
            }
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            tracing::info!(subscription_id = %subscription, job_id = %job, outcome = "replayed", elapsed_ms = started.elapsed().as_millis() as u64, "Podcast subscription accepted");
            return Ok(PodcastSubscriptionReceipt {
                request_id: request.request_id,
                subscription_id: identity(&subscription)?,
                job_id: identity(&job)?,
            });
        }
        check_request_capacity(&mut tx, actor.id()).await?;
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT id FROM podcast_subscriptions WHERE user_id=? AND feed_url=?",
        )
        .bind(actor.id())
        .bind(&url)
        .fetch_optional(&mut *tx)
        .await?;
        let mut reactivation_job = None;
        let subscription = if let Some(existing) = existing {
            let state = subscription_state(&mut tx, actor.id(), &existing).await?;
            if !state.active {
                reactivation_job =
                    Some(reactivate(&mut tx, actor.id(), &existing, state.generation).await?);
            }
            existing
        } else {
            check_subscription_capacity(&mut tx, actor.id()).await?;
            let library = crate::identity::personal_library(&mut tx, actor.id()).await?;
            let grant: String = sqlx::query_scalar(
                "SELECT id FROM library_access_grants WHERE library_id=? AND user_id=?",
            )
            .bind(&library)
            .bind(actor.id())
            .fetch_one(&mut *tx)
            .await?;
            let id = Uuid::new_v4().to_string();
            sqlx::query("INSERT INTO podcast_subscriptions(id,user_id,library_id,access_grant_id,feed_url,fetch_url) VALUES(?,?,?,?,?,?)")
                .bind(&id).bind(actor.id()).bind(library).bind(grant).bind(&url).bind(&url).execute(&mut *tx).await?;
            id
        };
        let outcome = if reactivation_job.is_some() {
            "reactivated"
        } else {
            "accepted"
        };
        let job = if let Some(job) = reactivation_job {
            job
        } else {
            let job: Option<String> = sqlx::query_scalar("SELECT id FROM podcast_refresh_jobs WHERE subscription_id=? ORDER BY created_at DESC,rowid DESC LIMIT 1")
                .bind(&subscription).fetch_optional(&mut *tx).await?;
            if let Some(job) = job {
                job
            } else {
                let generation: i64 =
                    sqlx::query_scalar("SELECT generation FROM podcast_subscriptions WHERE id=?")
                        .bind(&subscription)
                        .fetch_one(&mut *tx)
                        .await?;
                queue_refresh(&mut tx, actor.id(), &subscription, generation).await?
            }
        };
        sqlx::query("INSERT INTO podcast_subscription_receipts(user_id,request_id,request_digest,subscription_id,job_id) VALUES(?,?,?,?,?)")
            .bind(actor.id()).bind(request.request_id.to_string()).bind(digest).bind(&subscription).bind(&job).execute(&mut *tx).await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        tracing::info!(subscription_id = %subscription, job_id = %job, outcome, elapsed_ms = started.elapsed().as_millis() as u64, "Podcast subscription accepted");
        Ok(PodcastSubscriptionReceipt {
            request_id: request.request_id,
            subscription_id: identity(&subscription)?,
            job_id: identity(&job)?,
        })
    }

    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), request_id = %request.request_id))]
    pub async fn enqueue_refresh(
        &self,
        actor: &Actor,
        subscription: &str,
        request: RefreshPodcast,
    ) -> Result<PodcastRefreshReceipt> {
        let started = std::time::Instant::now();
        let subscription = Uuid::parse_str(subscription)
            .map_err(|_| Error::NotFound)?
            .to_string();
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let replay: Option<(String,String)> = sqlx::query_as("SELECT subscription_id,job_id FROM podcast_refresh_receipts WHERE user_id=? AND request_id=?")
            .bind(actor.id()).bind(request.request_id.to_string()).fetch_optional(&mut *tx).await?;
        if let Some((previous, job)) = replay {
            if previous != subscription {
                return Err(Error::Conflict(
                    "This request was already used for a different podcast".into(),
                ));
            }
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            return Ok(PodcastRefreshReceipt {
                request_id: request.request_id,
                subscription_id: identity(&subscription)?,
                job_id: identity(&job)?,
            });
        }
        check_request_capacity(&mut tx, actor.id()).await?;
        let generation: i64 = sqlx::query_scalar("SELECT p.generation FROM podcast_subscriptions p JOIN current_podcast_authority a ON a.id=p.id WHERE p.id=? AND p.user_id=?")
            .bind(&subscription).bind(actor.id()).fetch_optional(&mut *tx).await?.ok_or(Error::NotFound)?;
        let pending: Option<(String,i64)> = sqlx::query_as("SELECT id,generation FROM podcast_refresh_jobs WHERE subscription_id=? AND status IN ('queued','running')")
            .bind(&subscription).fetch_optional(&mut *tx).await?;
        let job = if let Some((job, pending_generation)) = pending {
            if pending_generation != generation {
                return Err(Error::Conflict(
                    "A previous podcast refresh is still finishing. Try again.".into(),
                ));
            }
            job
        } else {
            queue_refresh(&mut tx, actor.id(), &subscription, generation).await?
        };
        sqlx::query("INSERT INTO podcast_refresh_receipts(user_id,request_id,subscription_id,job_id) VALUES(?,?,?,?)")
            .bind(actor.id()).bind(request.request_id.to_string()).bind(&subscription).bind(&job).execute(&mut *tx).await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        tracing::info!(subscription_id = %subscription, job_id = %job, outcome = "queued", elapsed_ms = started.elapsed().as_millis() as u64, "Podcast refresh requested");
        Ok(PodcastRefreshReceipt {
            request_id: request.request_id,
            subscription_id: identity(&subscription)?,
            job_id: identity(&job)?,
        })
    }

    #[tracing::instrument(level = "debug", skip_all, err(Debug), fields(account_id = actor.id()))]
    pub async fn subscriptions(&self, actor: &Actor) -> Result<Vec<PodcastSubscription>> {
        actor.check_session(&self.store).await?;
        let rows: Vec<SubscriptionRow> = sqlx::query_as(
            "SELECT p.id,p.title,j.id IS NOT NULL AS refresh_pending,
                p.error IS NOT NULL AS refresh_failed,p.last_refreshed_at,p.content_revision
             FROM podcast_subscriptions p JOIN current_podcast_authority a ON a.id=p.id
             LEFT JOIN podcast_refresh_jobs j ON j.subscription_id=p.id
                AND j.generation=p.generation AND j.status IN ('queued','running')
             WHERE p.user_id=? ORDER BY p.created_at,p.id LIMIT 100",
        )
        .bind(actor.id())
        .fetch_all(&self.store.pool)
        .await?;
        let result = rows
            .into_iter()
            .map(|row| {
                Ok(PodcastSubscription {
                    subscription_id: identity(&row.id)?,
                    title: row.title,
                    refresh_pending: row.refresh_pending,
                    refresh_failed: row.refresh_failed,
                    last_refreshed_at: row.last_refreshed_at,
                    content_revision: row.content_revision,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        actor.check_session(&self.store).await?;
        Ok(result)
    }
}

pub(super) async fn check_request_capacity(
    connection: &mut sqlx::SqliteConnection,
    account: &str,
) -> Result<()> {
    let (owned,total): (i64,i64) = sqlx::query_as("SELECT coalesce((SELECT receipt_count FROM podcast_request_account_counts WHERE user_id=?),0),receipt_count FROM podcast_request_total WHERE singleton=1")
        .bind(account).fetch_one(connection).await?;
    if owned >= 4096 || total >= 65536 {
        return Err(Error::Conflict("Podcast request history is full".into()));
    }
    Ok(())
}

pub(super) async fn check_subscription_capacity(
    connection: &mut sqlx::SqliteConnection,
    account: &str,
) -> Result<()> {
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM podcast_subscriptions WHERE user_id=? AND active=1",
    )
    .bind(account)
    .fetch_one(connection)
    .await?;
    if count >= 100 {
        return Err(Error::Conflict(
            "This account already has 100 podcast subscriptions".into(),
        ));
    }
    Ok(())
}

pub(super) async fn queue_refresh(
    connection: &mut sqlx::SqliteConnection,
    account: &str,
    subscription: &str,
    generation: i64,
) -> Result<String> {
    let (owned, all): (i64, i64) = sqlx::query_as("SELECT coalesce(sum(user_id=?),0),count(*) FROM podcast_refresh_jobs WHERE status IN ('queued','running')")
        .bind(account).fetch_one(&mut *connection).await?;
    if owned >= 16 || all >= 128 {
        return Err(Error::Conflict(
            "Podcast refresh work is busy. Try again.".into(),
        ));
    }
    let job = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO podcast_refresh_jobs(id,subscription_id,user_id,generation,status) VALUES(?,?,?,?,'queued')")
        .bind(&job).bind(subscription).bind(account).bind(generation).execute(connection).await?;
    Ok(job)
}

pub(super) fn identity(value: &str) -> Result<Uuid> {
    Uuid::parse_str(value).map_err(|_| Error::Internal("Stored podcast identity is invalid".into()))
}
