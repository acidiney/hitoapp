use super::{AccountDeletion, Actor, DeleteAccount, Identity, account_data};
use crate::artist_catalog::erasure;
use crate::{Error, Result};
use std::time::Instant;
use uuid::Uuid;

impl Identity {
    #[tracing::instrument(level = "info", skip_all, err(Debug), fields(operation = "account_delete", request_id = %Uuid::new_v4()))]
    pub async fn delete_account(
        &self,
        actor: &Actor,
        account: &str,
        request: DeleteAccount,
    ) -> Result<AccountDeletion> {
        actor.require_admin()?;
        let started = Instant::now();
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let target: Option<(String, bool)> =
            sqlx::query_as("SELECT username, admin FROM users WHERE id = ?")
                .bind(account)
                .fetch_optional(&mut *tx)
                .await?;
        let (username, admin) = target.ok_or(Error::NotFound)?;
        if account == actor.id() {
            return Err(Error::Conflict(
                "Sign in as another administrator to delete this account".into(),
            ));
        }
        if username.trim() != request.username.trim() {
            return Err(Error::Conflict(
                "This account changed. Reload Settings.".into(),
            ));
        }
        if admin {
            let others: i64 =
                sqlx::query_scalar("SELECT count(*) FROM users WHERE admin = 1 AND id <> ?")
                    .bind(account)
                    .fetch_one(&mut *tx)
                    .await?;
            if others == 0 {
                return Err(Error::Conflict(
                    "The last administrator cannot be deleted".into(),
                ));
            }
        }
        let sessions_revoked: i64 =
            sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id = ?")
                .bind(account)
                .fetch_one(&mut *tx)
                .await?;
        let files_queued: i64 = sqlx::query_scalar("SELECT count(*) FROM download_files WHERE account_id = ? AND state IN ('staging','ready','deleting')")
            .bind(account)
            .fetch_one(&mut *tx)
            .await?;
        sqlx::query("PRAGMA defer_foreign_keys = ON")
            .execute(&mut *tx)
            .await?;
        let rows_removed =
            erasure::scoped(&mut tx, "account", account, &account_data::statements()).await?;
        tx.commit().await?;
        if let Err(error) = self.replicas.retire_pending().await {
            tracing::warn!(
                event = "replica_retirement_pending",
                error_code = error.code(),
                "Account replica file retirement will be retried"
            );
        }
        let deletion = AccountDeletion {
            sessions_revoked: sessions_revoked as u64,
            rows_removed,
            files_queued: files_queued as u64,
        };
        tracing::info!(
            outcome = "deleted",
            sessions_revoked = deletion.sessions_revoked,
            rows_removed = deletion.rows_removed,
            files_queued = deletion.files_queued,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Account deleted"
        );
        Ok(deletion)
    }
}
