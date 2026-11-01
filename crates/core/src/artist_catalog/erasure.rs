use crate::{Actor, ArtistCatalog, Error, Result};
use sqlx::SqliteConnection;
use std::time::Instant;
use uuid::Uuid;

const ASSOCIATIONS_OF_ACCOUNT: &str =
    "(SELECT id FROM recording_artist_associations WHERE user_id=?1)";

const PROVENANCE_TABLES: [&str; 7] = [
    "artist_association_matches",
    "artist_association_provider_matches",
    "artist_association_recording_id_matches",
    "artist_association_name_matches",
    "artist_association_credit_matches",
    "artist_association_coverage_matches",
    "artist_association_candidate_confirmations",
];

impl ArtistCatalog {
    /// Removes every private artist-correction decision of an account so that it can be deleted.
    /// Corrections an administrator shared stay in force and lose only their author.
    #[tracing::instrument(level = "info", skip_all, fields(operation = "artist_corrections_erase", request_id = %Uuid::new_v4()))]
    pub async fn erase_account_corrections(&self, actor: &Actor, account: &str) -> Result<()> {
        actor.require_admin()?;
        let started = Instant::now();
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        actor.check_session_on(&mut tx).await?;
        let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM users WHERE id=?")
            .bind(account)
            .fetch_optional(&mut *tx)
            .await?;
        exists.ok_or(Error::NotFound)?;
        erase_account(&mut tx, account).await?;
        tx.commit().await?;
        tracing::info!(
            outcome = "erased",
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Account artist corrections erased"
        );
        Ok(())
    }
}

async fn run(
    connection: &mut SqliteConnection,
    statements: &[String],
    subject: &str,
) -> Result<u64> {
    let mut removed = 0;
    for statement in statements {
        let affected = sqlx::query(statement)
            .bind(subject)
            .execute(&mut *connection)
            .await?
            .rows_affected();
        if statement.starts_with("DELETE FROM ")
            && !statement.starts_with("DELETE FROM correction_erasure_scopes")
        {
            removed += affected;
        }
    }
    Ok(removed)
}

/// Runs statements under an erasure scope and returns how many rows the deletes removed.
pub(crate) async fn scoped(
    connection: &mut SqliteConnection,
    kind: &str,
    subject: &str,
    statements: &[String],
) -> Result<u64> {
    sqlx::query("SAVEPOINT correction_erasure")
        .execute(&mut *connection)
        .await?;
    let outcome = erase_within_scope(connection, kind, subject, statements).await;
    if outcome.is_ok() {
        sqlx::query("RELEASE correction_erasure")
            .execute(&mut *connection)
            .await?;
    } else {
        for closing in [
            "ROLLBACK TO correction_erasure",
            "RELEASE correction_erasure",
        ] {
            if sqlx::query(closing)
                .execute(&mut *connection)
                .await
                .is_err()
            {
                tracing::warn!(
                    event = "correction_erasure_rollback_failed",
                    "Erasure scope cleanup failed; the original error is returned"
                );
            }
        }
    }
    outcome
}

async fn erase_within_scope(
    connection: &mut SqliteConnection,
    kind: &str,
    subject: &str,
    statements: &[String],
) -> Result<u64> {
    sqlx::query("INSERT INTO correction_erasure_scopes(kind,subject) VALUES(?,?)")
        .bind(kind)
        .bind(subject)
        .execute(&mut *connection)
        .await?;
    let removed = run(connection, statements, subject).await?;
    sqlx::query("DELETE FROM correction_erasure_scopes WHERE kind=? AND subject=?")
        .bind(kind)
        .bind(subject)
        .execute(&mut *connection)
        .await?;
    Ok(removed)
}

/// Removes scopes a failed or interrupted erasure could have left, which would otherwise
/// permit deletes of retained corrections.
pub(crate) async fn clear_stray_scopes(pool: &sqlx::SqlitePool) -> Result<()> {
    let cleared = sqlx::query("DELETE FROM correction_erasure_scopes")
        .execute(pool)
        .await?
        .rows_affected();
    if cleared > 0 {
        tracing::warn!(
            event = "correction_erasure_scopes_cleared",
            cleared,
            "Cleared erasure scopes left by an interrupted erasure"
        );
    }
    Ok(())
}

fn provenance(associations: &str) -> Vec<String> {
    PROVENANCE_TABLES
        .iter()
        .map(|table| format!("DELETE FROM {table} WHERE association_id IN {associations}"))
        .collect()
}

pub(crate) async fn erase_account(connection: &mut SqliteConnection, account: &str) -> Result<u64> {
    scoped(connection, "account", account, &account_statements()).await
}

pub(crate) fn account_statements() -> Vec<String> {
    let mut statements: Vec<String> = [
        "DELETE FROM recording_artist_identity_choices WHERE user_id=?1",
        "DELETE FROM album_edition_choices WHERE user_id=?1",
        "DELETE FROM artist_match_dismissals WHERE user_id=?1",
        "DELETE FROM artist_match_checks WHERE user_id=?1",
        "DELETE FROM artist_provider_checks WHERE user_id=?1",
        "DELETE FROM artist_name_checks WHERE user_id=?1",
        "DELETE FROM artist_name_candidates WHERE user_id=?1",
        "DELETE FROM artist_review_dismissals WHERE user_id=?1",
        "UPDATE shared_artist_correction_states SET changed_by=NULL WHERE changed_by=?1",
        "UPDATE shared_artist_corrections SET promoted_by=NULL,source_change_id=NULL,source_association_id=NULL WHERE promoted_by=?1",
        "DELETE FROM artist_identity_change_items WHERE change_id IN (SELECT id FROM artist_identity_changes WHERE user_id=?1)",
        "DELETE FROM artist_identity_changes WHERE user_id=?1",
    ]
    .map(String::from)
    .into();
    statements.extend(provenance(ASSOCIATIONS_OF_ACCOUNT));
    statements.extend(
        [
            "DELETE FROM artist_association_change_retirements WHERE user_id=?1",
            "DELETE FROM recording_artist_association_change_order WHERE user_id=?1",
            "DELETE FROM recording_artist_association_changes WHERE user_id=?1",
            "DELETE FROM recording_artist_associations WHERE user_id=?1",
            "DELETE FROM account_artist_identity_groups WHERE user_id=?1",
            "DELETE FROM artist_identity_choice_receipts WHERE user_id=?1",
            "DELETE FROM recording_artist_association_receipts WHERE user_id=?1",
            "DELETE FROM album_edition_choice_receipts WHERE user_id=?1",
            "DELETE FROM shared_artist_correction_receipts WHERE user_id=?1",
        ]
        .map(String::from),
    );
    statements
}

/// Called by the transaction that removes a recording, before the recording row itself.
pub(crate) async fn erase_recording_corrections(
    connection: &mut SqliteConnection,
    recording: &str,
) -> Result<()> {
    scoped(
        connection,
        "recording",
        recording,
        &recording_statements("(?1)"),
    )
    .await
    .map(|_| ())
}

/// Statements that remove every correction naming the recordings selected by `recordings`, a
/// parenthesized list or subquery. The caller holds a `recording` scope for each of them.
pub(crate) fn recording_statements(recordings: &str) -> Vec<String> {
    let associations = format!(
        "(SELECT id FROM recording_artist_associations WHERE recording_id IN {recordings})"
    );
    let mut statements: Vec<String> = [
        "DELETE FROM recording_artist_identity_choices",
        "DELETE FROM album_edition_choices",
        "DELETE FROM artist_match_checks",
        "DELETE FROM artist_provider_checks",
        "DELETE FROM artist_name_checks",
        "DELETE FROM artist_identity_change_items",
    ]
    .map(|statement| format!("{statement} WHERE recording_id IN {recordings}"))
    .into();
    statements.extend(provenance(&associations));
    statements.push(format!(
        "UPDATE shared_artist_corrections SET source_association_id=NULL WHERE source_association_id IN {associations}"
    ));
    statements.push(format!(
        "DELETE FROM artist_association_change_retirements WHERE association_id IN {associations}"
    ));
    statements.extend(
        [
            "DELETE FROM recording_artist_association_change_order",
            "DELETE FROM recording_artist_association_changes",
            "DELETE FROM recording_artist_associations",
            "DELETE FROM shared_artist_correction_subjects",
            "DELETE FROM recording_isrc_observations",
            "DELETE FROM recording_artist_url_observations",
            "DELETE FROM recording_musicbrainz_observations",
        ]
        .map(|statement| format!("{statement} WHERE recording_id IN {recordings}")),
    );
    statements
}

#[cfg(test)]
mod tests {
    use crate::store::Store;

    #[tokio::test]
    async fn opening_the_store_clears_stray_erasure_scopes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("scopes.db");
        let store = Store::open(&path).await.unwrap();
        sqlx::query(
            "INSERT INTO correction_erasure_scopes(kind,subject) VALUES('account','stray')",
        )
        .execute(&store.pool)
        .await
        .unwrap();
        store.pool.close().await;
        let store = Store::open(&path).await.unwrap();
        let scopes: i64 = sqlx::query_scalar("SELECT count(*) FROM correction_erasure_scopes")
            .fetch_one(&store.pool)
            .await
            .unwrap();
        assert_eq!(scopes, 0);
    }
}
