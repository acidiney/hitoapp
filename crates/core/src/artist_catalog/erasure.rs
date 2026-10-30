use crate::{Actor, ArtistCatalog, Error, Result};
use sqlx::SqliteConnection;
use std::time::Instant;
use uuid::Uuid;

const ASSOCIATIONS_OF_RECORDING: &str =
    "(SELECT id FROM recording_artist_associations WHERE recording_id=?1)";
const ASSOCIATIONS_OF_ACCOUNT: &str =
    "(SELECT id FROM recording_artist_associations WHERE user_id=?1)";

const PROVENANCE_TABLES: [&str; 3] = [
    "artist_association_matches",
    "artist_association_provider_matches",
    "artist_association_name_matches",
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
) -> Result<()> {
    for statement in statements {
        sqlx::query(statement)
            .bind(subject)
            .execute(&mut *connection)
            .await?;
    }
    Ok(())
}

async fn scoped(
    connection: &mut SqliteConnection,
    kind: &str,
    subject: &str,
    statements: &[String],
) -> Result<()> {
    sqlx::query("INSERT INTO correction_erasure_scopes(kind,subject) VALUES(?,?)")
        .bind(kind)
        .bind(subject)
        .execute(&mut *connection)
        .await?;
    run(connection, statements, subject).await?;
    sqlx::query("DELETE FROM correction_erasure_scopes WHERE kind=? AND subject=?")
        .bind(kind)
        .bind(subject)
        .execute(&mut *connection)
        .await?;
    Ok(())
}

fn provenance(associations: &str) -> Vec<String> {
    PROVENANCE_TABLES
        .iter()
        .map(|table| format!("DELETE FROM {table} WHERE association_id IN {associations}"))
        .collect()
}

pub(crate) async fn erase_account(connection: &mut SqliteConnection, account: &str) -> Result<()> {
    let mut statements: Vec<String> = [
        "DELETE FROM recording_artist_identity_choices WHERE user_id=?1",
        "DELETE FROM album_edition_choices WHERE user_id=?1",
        "DELETE FROM artist_match_dismissals WHERE user_id=?1",
        "DELETE FROM artist_match_checks WHERE user_id=?1",
        "DELETE FROM artist_provider_checks WHERE user_id=?1",
        "DELETE FROM artist_name_checks WHERE user_id=?1",
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
    scoped(connection, "account", account, &statements).await
}

/// Called by the transaction that removes a recording, before the recording row itself.
pub(crate) async fn erase_recording_corrections(
    connection: &mut SqliteConnection,
    recording: &str,
) -> Result<()> {
    let mut statements: Vec<String> = [
        "DELETE FROM recording_artist_identity_choices WHERE recording_id=?1",
        "DELETE FROM album_edition_choices WHERE recording_id=?1",
        "DELETE FROM artist_match_checks WHERE recording_id=?1",
        "DELETE FROM artist_provider_checks WHERE recording_id=?1",
        "DELETE FROM artist_name_checks WHERE recording_id=?1",
        "DELETE FROM artist_identity_change_items WHERE recording_id=?1",
    ]
    .map(String::from)
    .into();
    statements.extend(provenance(ASSOCIATIONS_OF_RECORDING));
    statements.push(format!(
        "UPDATE shared_artist_corrections SET source_association_id=NULL WHERE source_association_id IN {ASSOCIATIONS_OF_RECORDING}"
    ));
    statements.push(format!(
        "DELETE FROM artist_association_change_retirements WHERE association_id IN {ASSOCIATIONS_OF_RECORDING}"
    ));
    statements.extend(
        [
            "DELETE FROM recording_artist_association_change_order WHERE recording_id=?1",
            "DELETE FROM recording_artist_association_changes WHERE recording_id=?1",
            "DELETE FROM recording_artist_associations WHERE recording_id=?1",
            "DELETE FROM shared_artist_correction_subjects WHERE recording_id=?1",
            "DELETE FROM recording_isrc_observations WHERE recording_id=?1",
            "DELETE FROM recording_artist_url_observations WHERE recording_id=?1",
            "DELETE FROM recording_musicbrainz_observations WHERE recording_id=?1",
        ]
        .map(String::from),
    );
    scoped(connection, "recording", recording, &statements).await
}
