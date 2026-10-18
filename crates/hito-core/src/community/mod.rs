mod config;
mod reports;
mod source;
mod transport;
mod types;
mod worker;

use crate::{Actor, Error, HostDeviceId, Result, secrets::Vault, store::Store};
pub use config::CommunityConfig;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
pub use types::{
    CommunityCredentialReset, CommunityPreferences, CommunityReportAdmission, SuccessfulPlayback,
    UpdateCommunityPreferences,
};
use uuid::Uuid;

#[derive(Clone)]
pub struct Community {
    store: Store,
    host: HostDeviceId,
    vault: Vault,
    remote: Option<Arc<transport::Transport>>,
    changed: tokio::sync::watch::Sender<u64>,
}

impl Community {
    #[tracing::instrument(level = "info", skip_all, fields(operation = "community_credentials_reset", request_id = %Uuid::new_v4()))]
    pub async fn reset_credentials(&self, actor: &Actor) -> Result<CommunityCredentialReset> {
        let started = Instant::now();
        let result = tokio::time::timeout(Duration::from_secs(1), async {
            let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
            actor.check_session_on(&mut tx).await?;
            let reports_cancelled = sqlx::query(
                "UPDATE community_reports SET status='cancelled',lease_token=NULL,lease_until=NULL,updated_at=unixepoch()
                 WHERE user_id=? AND status IN ('queued','running')",
            )
            .bind(actor.id())
            .execute(&mut *tx)
            .await?
            .rows_affected();
            let reporters_removed = sqlx::query("DELETE FROM community_reporters WHERE user_id=?")
                .bind(actor.id())
                .execute(&mut *tx)
                .await?
                .rows_affected();
            let reset = CommunityCredentialReset {
                reporters_removed: u32::try_from(reporters_removed).map_err(|_| {
                    Error::Internal("Community credential reset exceeded its count limit".into())
                })?,
                reports_cancelled: u32::try_from(reports_cancelled).map_err(|_| {
                    Error::Internal("Community credential reset exceeded its count limit".into())
                })?,
            };
            actor.check_session_on(&mut tx).await?;
            tx.commit().await?;
            self.changed.send_modify(|generation| *generation = generation.wrapping_add(1));
            actor.check_session(&self.store).await?;
            Ok(reset)
        })
        .await
        .unwrap_or(Err(Error::Upstream("Community credentials could not be reset in time")));
        tracing::info!(
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |_| "reset"),
            reporters_removed = result.as_ref().map_or(0, |reset| reset.reporters_removed),
            reports_cancelled = result.as_ref().map_or(0, |reset| reset.reports_cancelled),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Community credentials reset completed"
        );
        result
    }

    pub(crate) fn new(
        store: Store,
        host: HostDeviceId,
        vault: Vault,
        config: Option<CommunityConfig>,
    ) -> Result<Self> {
        Ok(Self {
            store,
            host,
            vault,
            remote: config
                .map(transport::Transport::new)
                .transpose()?
                .map(Arc::new),
            changed: tokio::sync::watch::channel(0).0,
        })
    }

    #[tracing::instrument(level = "debug", skip_all, fields(operation = "community_preferences", request_id = %Uuid::new_v4()))]
    pub async fn preferences(&self, actor: &Actor) -> Result<CommunityPreferences> {
        let started = Instant::now();
        let result = tokio::time::timeout(Duration::from_secs(1), async {
            let mut tx = self.store.pool.begin().await?;
            actor.check_session_on(&mut tx).await?;
            let preferences = read(&mut tx, actor).await?;
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            Ok(preferences)
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Community preferences could not be read in time",
        )));
        tracing::debug!(
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |_| "success"),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Community preferences read completed"
        );
        result
    }

    #[tracing::instrument(level = "info", skip_all, fields(operation = "community_preferences_update", request_id = %Uuid::new_v4()))]
    pub async fn update_preferences(
        &self,
        actor: &Actor,
        request: UpdateCommunityPreferences,
    ) -> Result<CommunityPreferences> {
        let started = Instant::now();
        let result = tokio::time::timeout(Duration::from_secs(1), async {
            let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
            actor.check_session_on(&mut tx).await?;
            let current = read(&mut tx, actor).await?;
            if current.revision != request.revision {
                return Err(Error::Conflict(
                    "Community preferences changed. Review them again.".into(),
                ));
            }
            let revision = current.revision.checked_add(1).ok_or_else(|| {
                Error::Conflict("Community preferences cannot be changed further".into())
            })?;
            sqlx::query("INSERT INTO account_community_preferences(user_id,revision,playback_reporting,metadata_corrections) VALUES (?,?,?,?)
                ON CONFLICT(user_id) DO UPDATE SET revision=excluded.revision,playback_reporting=excluded.playback_reporting,metadata_corrections=excluded.metadata_corrections")
                .bind(actor.id()).bind(revision).bind(request.playback_reporting).bind(request.metadata_corrections)
                .execute(&mut *tx).await?;
            actor.check_session_on(&mut tx).await?;
            tx.commit().await?;
            actor.check_session(&self.store).await?;
            self.changed.send_modify(|generation| *generation = generation.wrapping_add(1));
            Ok(CommunityPreferences {
                revision,
                playback_reporting: request.playback_reporting,
                metadata_corrections: request.metadata_corrections,
            })
        })
        .await
        .unwrap_or(Err(Error::Upstream(
            "Community preferences could not be saved in time",
        )));
        tracing::info!(
            outcome = result
                .as_ref()
                .map_or_else(|error| error.code(), |_| "success"),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Community preferences update completed"
        );
        result
    }
}

async fn read(
    connection: &mut sqlx::SqliteConnection,
    actor: &Actor,
) -> Result<CommunityPreferences> {
    Ok(sqlx::query_as(
        "SELECT revision,playback_reporting,metadata_corrections FROM account_community_preferences WHERE user_id=?",
    )
    .bind(actor.id())
    .fetch_optional(connection)
    .await?
    .unwrap_or_default())
}
