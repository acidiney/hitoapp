use crate::Result;
use sqlx::{QueryBuilder, Sqlite, Transaction};

pub(crate) enum Scope<'a> {
    Recording(&'a str),
    Folder(&'a str),
    Library { user: &'a str, library: &'a str },
}

impl Scope<'_> {
    fn push_filter(&self, query: &mut QueryBuilder<'static, Sqlite>, user_column: Option<&str>) {
        match self {
            Scope::Recording(recording) => {
                query.push(" AND r.id = ").push_bind(recording.to_string());
            }
            Scope::Folder(folder) => {
                query.push(" AND r.id IN (SELECT s.recording_id FROM local_files f JOIN sources s ON s.id = f.source_id WHERE f.folder_id = ")
                    .push_bind(folder.to_string()).push(")");
            }
            Scope::Library { user, library } => {
                if let Some(column) = user_column {
                    query
                        .push(" AND ")
                        .push(column)
                        .push(" = ")
                        .push_bind(user.to_string());
                }
                query
                    .push(" AND r.library_id = ")
                    .push_bind(library.to_string());
            }
        }
        query.push(") ");
    }

    /// A `WITH targets(recording_id)` prefix naming the recordings in this scope.
    pub(crate) fn recordings(&self) -> QueryBuilder<'static, Sqlite> {
        let mut query = QueryBuilder::new(
            "WITH targets AS MATERIALIZED (SELECT r.id AS recording_id FROM recordings r WHERE 1",
        );
        self.push_filter(&mut query, None);
        query
    }

    fn account_recordings(&self) -> QueryBuilder<'static, Sqlite> {
        let mut query = QueryBuilder::new(
            "WITH targets AS (SELECT access.user_id, r.library_id, r.id AS recording_id,
            COALESCE(p.override_artist, credit.credit_label, r.artist) AS name,
            CASE WHEN p.override_artist IS NULL THEN credit.credit_key END AS credit_key,
            credit.rowid AS credit_order,
            (SELECT o.artist FROM metadata_observations o WHERE o.id = r.metadata_observation_id) AS observed,
            EXISTS (SELECT 1 FROM recording_credited_artist_observations f
                WHERE f.observation_id = credit.observation_id AND f.role = 'featured') AS featured
         FROM recordings r JOIN library_members access ON access.library_id = r.library_id
         LEFT JOIN personal_recording_metadata p ON p.user_id = access.user_id AND p.recording_id = r.id
         LEFT JOIN recording_credit_observations credit ON credit.observation_id = r.metadata_observation_id WHERE 1",
        );
        self.push_filter(&mut query, Some("access.user_id"));
        query
    }

    fn chosen_recordings(&self) -> QueryBuilder<'static, Sqlite> {
        let mut query = QueryBuilder::new(
            "WITH targets AS MATERIALIZED (SELECT choice.user_id, r.id AS recording_id
         FROM recordings r JOIN recording_artist_identity_choices choice ON choice.recording_id=r.id
         WHERE choice.retired=0 AND choice.association_id IS NOT NULL",
        );
        self.push_filter(&mut query, Some("choice.user_id"));
        query
    }
}

const REGISTER_TEXT_CREDITS: &str = "INSERT INTO account_artists(user_id, id, library_id, name)
        SELECT t.user_id, COALESCE(shared.id, lower(hex(randomblob(16)))), t.library_id, t.name FROM targets t
        LEFT JOIN artists shared ON shared.library_id = t.library_id AND shared.name = t.name COLLATE BINARY
        WHERE t.name IS NOT NULL AND t.credit_key IS NULL
        ON CONFLICT(user_id, library_id, name) DO NOTHING";

const FIRST_SEEN: &str = "(SELECT t.*, ROW_NUMBER() OVER (PARTITION BY t.user_id, t.library_id, t.credit_key ORDER BY t.credit_order) AS first_seen
        FROM targets t WHERE t.credit_key IS NOT NULL)";

fn adopt_text_credits() -> String {
    format!(
        "INSERT OR IGNORE INTO account_credit_keys(user_id, library_id, credit_key, artist_id)
        SELECT t.user_id, t.library_id, t.credit_key, a.id FROM {first_seen} t
        JOIN account_artists a ON a.user_id = t.user_id AND a.library_id = t.library_id AND a.name = t.observed COLLATE BINARY
        WHERE t.first_seen = 1 AND t.featured = 0
            AND NOT EXISTS (SELECT 1 FROM account_credit_keys k
                WHERE k.user_id = t.user_id AND k.library_id = t.library_id AND k.credit_key = t.credit_key)
            AND NOT EXISTS (SELECT 1 FROM account_credit_keys k WHERE k.user_id = a.user_id AND k.artist_id = a.id)",
        first_seen = FIRST_SEEN
    )
}

fn register_keyed_credits() -> String {
    format!(
        "INSERT INTO account_artists(user_id, id, library_id, name)
        SELECT t.user_id, COALESCE(shared.id, lower(hex(randomblob(16)))), t.library_id, t.name FROM {first_seen} t
        LEFT JOIN artists shared ON shared.library_id = t.library_id AND shared.name = t.name COLLATE BINARY
        WHERE t.first_seen = 1 AND NOT EXISTS (SELECT 1 FROM account_credit_keys k
            WHERE k.user_id = t.user_id AND k.library_id = t.library_id AND k.credit_key = t.credit_key)
        ON CONFLICT(user_id, library_id, name) DO NOTHING",
        first_seen = FIRST_SEEN
    )
}

fn record_credit_keys() -> String {
    format!("INSERT OR IGNORE INTO account_credit_keys(user_id, library_id, credit_key, artist_id)
        SELECT t.user_id, t.library_id, t.credit_key, a.id FROM {first_seen} t
        JOIN account_artists a ON a.user_id = t.user_id AND a.library_id = t.library_id AND a.name = t.name COLLATE BINARY
        WHERE t.first_seen = 1",
        first_seen = FIRST_SEEN
    )
}

#[tracing::instrument(level = "trace", skip_all, err(Debug))]
pub(super) async fn refresh(tx: &mut Transaction<'_, Sqlite>, scope: Scope<'_>) -> Result<()> {
    let mut register = scope.account_recordings();
    register.push(REGISTER_TEXT_CREDITS);
    register.build().execute(&mut **tx).await?;
    let mut adopt = scope.account_recordings();
    adopt.push(adopt_text_credits());
    adopt.build().execute(&mut **tx).await?;
    let mut keyed = scope.account_recordings();
    keyed.push(register_keyed_credits());
    let created = keyed.build().execute(&mut **tx).await?.rows_affected();
    let mut keys = scope.account_recordings();
    keys.push(record_credit_keys());
    keys.build().execute(&mut **tx).await?;
    let mut bind = scope.account_recordings();
    bind.push("INSERT INTO account_recording_credits(user_id, recording_id, artist_id)
        SELECT user_id, recording_id, artist_id FROM (
            SELECT t.user_id, t.recording_id, COALESCE(
                (SELECT k.artist_id FROM account_credit_keys k WHERE k.user_id = t.user_id AND k.library_id = t.library_id AND k.credit_key = t.credit_key),
                (SELECT c.id FROM account_artists c WHERE c.user_id = t.user_id AND c.library_id = t.library_id AND c.name = t.name COLLATE BINARY)) AS artist_id
            FROM targets t) WHERE artist_id IS NOT NULL
        ON CONFLICT(user_id, recording_id) DO UPDATE SET artist_id = excluded.artist_id
        WHERE artist_id IS NOT excluded.artist_id");
    let changed = bind.build().execute(&mut **tx).await?.rows_affected();
    if created > 0 {
        let mut accounts = scope.account_recordings();
        accounts.push("SELECT DISTINCT user_id FROM targets");
        let accounts: Vec<String> = accounts.build_query_scalar().fetch_all(&mut **tx).await?;
        for account in accounts {
            crate::artist_catalog::automation::request_for_account(tx, &account).await?;
        }
    }
    crate::artist_catalog::identity::retire_changed(tx, scope.chosen_recordings()).await?;
    crate::artist_catalog::reconcile::retire_stale(tx, &scope).await?;
    tracing::trace!(changed, "Account recording credits refreshed");
    Ok(())
}

pub(crate) async fn recording(tx: &mut Transaction<'_, Sqlite>, recording: &str) -> Result<()> {
    refresh(tx, Scope::Recording(recording)).await
}

pub(crate) async fn folder(tx: &mut Transaction<'_, Sqlite>, folder: &str) -> Result<()> {
    refresh(tx, Scope::Folder(folder)).await
}
