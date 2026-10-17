use super::{
    PodcastUnsubscribeReceipt, Podcasts, UnsubscribePodcast,
    subscriptions::{check_request_capacity, check_subscription_capacity, identity, queue_refresh},
};
use crate::{Actor, Error, Result};
use sqlx::SqliteConnection;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
pub(super) struct SubscriptionState {
    pub(super) active: bool,
    pub(super) generation: i64,
}

impl Podcasts {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(account_id = actor.id(), request_id = %request.request_id))]
    pub async fn unsubscribe(
        &self,
        actor: &Actor,
        subscription: &str,
        request: UnsubscribePodcast,
    ) -> Result<PodcastUnsubscribeReceipt> {
        let started = std::time::Instant::now();
        let subscription = Uuid::parse_str(subscription)
            .map_err(|_| Error::NotFound)?
            .to_string();
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let state = subscription_state(&mut tx, actor.id(), &subscription).await?;
        let replay: Option<String> = sqlx::query_scalar(
            "SELECT subscription_id FROM podcast_unsubscribe_receipts WHERE user_id=? AND request_id=?",
        )
        .bind(actor.id())
        .bind(request.request_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(previous) = replay {
            if previous != subscription {
                return Err(Error::Conflict(
                    "This request was already used for a different podcast".into(),
                ));
            }
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            tracing::info!(subscription_id = %subscription, outcome = "replayed", elapsed_ms = started.elapsed().as_millis() as u64, "Podcast unsubscribe acknowledged");
            return Ok(PodcastUnsubscribeReceipt {
                request_id: request.request_id,
                subscription_id: identity(&subscription)?,
            });
        }
        check_request_capacity(&mut tx, actor.id()).await?;
        if state.active {
            let generation = next_generation(state.generation)?;
            sqlx::query(
                "UPDATE podcast_subscriptions SET active=0,generation=? WHERE id=? AND user_id=?",
            )
            .bind(generation)
            .bind(&subscription)
            .bind(actor.id())
            .execute(&mut *tx)
            .await?;
            cancel_queued(&mut tx, &subscription).await?;
        }
        sqlx::query("INSERT INTO podcast_unsubscribe_receipts(user_id,request_id,subscription_id) VALUES(?,?,?)")
            .bind(actor.id())
            .bind(request.request_id.to_string())
            .bind(&subscription)
            .execute(&mut *tx)
            .await?;
        actor.check_session_on(&mut tx).await?;
        tx.commit().await?;
        actor.check_session(&self.store).await?;
        let outcome = if state.active {
            "unsubscribed"
        } else {
            "already_inactive"
        };
        tracing::info!(subscription_id = %subscription, outcome, elapsed_ms = started.elapsed().as_millis() as u64, "Podcast unsubscribe acknowledged");
        Ok(PodcastUnsubscribeReceipt {
            request_id: request.request_id,
            subscription_id: identity(&subscription)?,
        })
    }
}

pub(super) async fn subscription_state(
    connection: &mut SqliteConnection,
    account: &str,
    subscription: &str,
) -> Result<SubscriptionState> {
    sqlx::query_as(
        "SELECT p.active,p.generation FROM podcast_subscriptions p
         JOIN libraries l ON l.id=p.library_id AND l.owner_id=p.user_id
         JOIN library_access_grants g ON g.library_id=p.library_id AND g.user_id=p.user_id AND g.id=p.access_grant_id
         WHERE p.id=? AND p.user_id=?",
    )
    .bind(subscription)
    .bind(account)
    .fetch_optional(connection)
    .await?
    .ok_or(Error::NotFound)
}

pub(super) async fn reactivate(
    connection: &mut SqliteConnection,
    account: &str,
    subscription: &str,
    generation: i64,
) -> Result<String> {
    let pending: Option<String> = sqlx::query_scalar(
        "SELECT status FROM podcast_refresh_jobs WHERE subscription_id=? AND status IN ('queued','running')",
    )
    .bind(subscription)
    .fetch_optional(&mut *connection)
    .await?;
    if pending.as_deref() == Some("running") {
        return Err(Error::Conflict(
            "A previous podcast refresh is still finishing. Try again.".into(),
        ));
    }
    check_subscription_capacity(connection, account).await?;
    let generation = next_generation(generation)?;
    cancel_queued(connection, subscription).await?;
    sqlx::query("UPDATE podcast_subscriptions SET active=1,generation=?,error=NULL,next_refresh_at=unixepoch() WHERE id=? AND user_id=?")
        .bind(generation)
        .bind(subscription)
        .bind(account)
        .execute(&mut *connection)
        .await?;
    queue_refresh(connection, account, subscription, generation).await
}

async fn cancel_queued(connection: &mut SqliteConnection, subscription: &str) -> Result<()> {
    sqlx::query("UPDATE podcast_refresh_jobs SET status='cancelled',error='authority_changed',updated_at=unixepoch() WHERE subscription_id=? AND status IN ('queued','running') AND status='queued'")
        .bind(subscription)
        .execute(connection)
        .await?;
    Ok(())
}

fn next_generation(generation: i64) -> Result<i64> {
    generation
        .checked_add(1)
        .ok_or_else(|| Error::Conflict("This podcast subscription cannot change again".into()))
}
