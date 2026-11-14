use crate::Result;
use sqlx::{QueryBuilder, Sqlite, Transaction};
use std::time::Instant;

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
            credit.rowid AS credit_order, credit.observation_id AS credit_observation,
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
    fn member_chosen_recordings(&self) -> QueryBuilder<'static, Sqlite> {
        let mut query = QueryBuilder::new(
            "WITH targets AS MATERIALIZED (SELECT choice.user_id,r.id AS recording_id
             FROM recordings r JOIN recording_artist_member_identity_choices choice ON choice.recording_id=r.id
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

const MEMBERS: &str = "(SELECT t.user_id, t.library_id, t.recording_id, member.name, member.role FROM targets t
        JOIN recording_credited_artist_observations member ON member.observation_id = t.credit_observation
        WHERE t.credit_key IS NOT NULL)";

const REGISTER_MEMBER_ARTISTS: &str = "INSERT INTO account_artists(user_id, id, library_id, name)
        SELECT m.user_id, COALESCE(shared.id, lower(hex(randomblob(16)))), m.library_id, m.name FROM MEMBERS m
        LEFT JOIN artists shared ON shared.library_id = m.library_id AND shared.name = m.name COLLATE BINARY
        ON CONFLICT(user_id, library_id, name) DO NOTHING";

const BIND_MEMBERS: &str = "INSERT OR IGNORE INTO account_recording_credit_members(user_id, recording_id, artist_id, role)
        SELECT m.user_id, m.recording_id, a.id, m.role FROM MEMBERS m
        JOIN account_artists a ON a.user_id = m.user_id AND a.library_id = m.library_id AND a.name = m.name COLLATE BINARY";

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
    let mut unbind_members = scope.account_recordings();
    unbind_members.push(
        "DELETE FROM account_recording_credit_members
         WHERE (user_id, recording_id) IN (SELECT user_id, recording_id FROM targets)",
    );
    unbind_members.build().execute(&mut **tx).await?;
    let mut member_artists = scope.account_recordings();
    member_artists.push(REGISTER_MEMBER_ARTISTS.replace("MEMBERS", MEMBERS));
    member_artists.build().execute(&mut **tx).await?;
    let mut members = scope.account_recordings();
    members.push(BIND_MEMBERS.replace("MEMBERS", MEMBERS));
    members.build().execute(&mut **tx).await?;
    if created > 0 {
        let mut accounts = scope.account_recordings();
        accounts.push("SELECT DISTINCT user_id FROM targets");
        let accounts: Vec<String> = accounts.build_query_scalar().fetch_all(&mut **tx).await?;
        for account in accounts {
            crate::artist_catalog::automation::request_for_account(tx, &account).await?;
        }
    }
    crate::artist_catalog::identity::retire_changed(tx, scope.chosen_recordings()).await?;
    crate::artist_catalog::identity::retire_member_choices(tx, scope.member_chosen_recordings())
        .await?;
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

#[derive(sqlx::FromRow)]
struct MissingMainMember {
    user_id: String,
    library_id: String,
    recording_id: String,
    name: String,
}

const REPAIR_CANDIDATES: &str = "WITH missing AS MATERIALIZED (
    SELECT access.user_id, recording.library_id, recording.id AS recording_id,
        credit.observation_id, main.name
    FROM recordings recording
    CROSS JOIN library_members access
    CROSS JOIN recording_credit_observations credit
    CROSS JOIN account_recording_credits whole
    CROSS JOIN recording_credited_artist_observations main
    LEFT JOIN personal_recording_metadata personal ON personal.user_id=access.user_id
        AND personal.recording_id=recording.id
    WHERE access.library_id=recording.library_id
        AND credit.recording_id=recording.id
        AND credit.observation_id=recording.metadata_observation_id
        AND whole.user_id=access.user_id AND whole.recording_id=recording.id
        AND main.observation_id=credit.observation_id
        AND main.role='main'
        AND personal.override_artist IS NULL
        AND NOT EXISTS (SELECT 1 FROM account_recording_credit_members member
            JOIN account_artists artist ON artist.user_id=member.user_id AND artist.id=member.artist_id
                AND artist.library_id=recording.library_id
            WHERE member.user_id=access.user_id AND member.recording_id=recording.id
                AND member.role='main' AND artist.name=main.name COLLATE BINARY)
)
SELECT missing.user_id,missing.library_id,missing.recording_id,missing.name
FROM missing
WHERE (SELECT count(*) FROM recording_credited_artist_observations one_main
    WHERE one_main.observation_id=missing.observation_id
        AND one_main.role='main')=1";

const REPAIR_CANDIDATE_TABLE: &str = "CREATE TEMP TABLE member_repair_candidates(
    user_id TEXT NOT NULL,library_id TEXT NOT NULL,recording_id TEXT NOT NULL,name TEXT NOT NULL,
    PRIMARY KEY(user_id,recording_id)) WITHOUT ROWID";

const REGISTER_REPAIR_ARTISTS: &str = "INSERT INTO account_artists(user_id,id,library_id,name)
    SELECT candidate.user_id,COALESCE(shared.id,lower(hex(randomblob(16)))),candidate.library_id,candidate.name
    FROM member_repair_candidates candidate LEFT JOIN artists shared
        ON shared.library_id=candidate.library_id AND shared.name=candidate.name COLLATE BINARY
    WHERE 1 ON CONFLICT(user_id,library_id,name) DO NOTHING";

const BIND_REPAIR_MEMBERS: &str =
    "INSERT OR IGNORE INTO account_recording_credit_members(user_id,recording_id,artist_id,role)
    SELECT candidate.user_id,candidate.recording_id,artist.id,'main'
    FROM member_repair_candidates candidate JOIN account_artists artist
        ON artist.user_id=candidate.user_id AND artist.library_id=candidate.library_id
            AND artist.name=candidate.name COLLATE BINARY";

#[tracing::instrument(level = "info", skip_all, err(Debug))]
pub(crate) async fn repair_missing_main_members(pool: &sqlx::SqlitePool) -> Result<()> {
    let started = Instant::now();
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let candidates: Vec<MissingMainMember> = sqlx::query_as(REPAIR_CANDIDATES)
        .fetch_all(&mut *tx)
        .await?;
    if !candidates.is_empty() {
        sqlx::query(REPAIR_CANDIDATE_TABLE)
            .execute(&mut *tx)
            .await?;
        for batch in candidates.chunks(256) {
            let mut insert = QueryBuilder::<Sqlite>::new(
                "INSERT INTO member_repair_candidates(user_id,library_id,recording_id,name) ",
            );
            insert.push_values(batch, |mut row, candidate| {
                row.push_bind(&candidate.user_id)
                    .push_bind(&candidate.library_id)
                    .push_bind(&candidate.recording_id)
                    .push_bind(&candidate.name);
            });
            insert.build().execute(&mut *tx).await?;
        }
        sqlx::query(REGISTER_REPAIR_ARTISTS)
            .execute(&mut *tx)
            .await?;
        let repaired = sqlx::query(BIND_REPAIR_MEMBERS)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        sqlx::query("DROP TABLE member_repair_candidates")
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        tracing::info!(
            candidates = candidates.len(),
            repaired,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Historical main credit members repaired"
        );
    } else {
        tx.commit().await?;
        tracing::debug!(
            candidates = 0,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "Historical main credit member repair found no work"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::work_meter::WorkMeter;
    use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};

    const VM_STEP_QUANTUM: u64 = 100;

    async fn pool(history: i64, existing_main: bool) -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::raw_sql(
            "CREATE TABLE recordings(id TEXT PRIMARY KEY,library_id TEXT NOT NULL,metadata_observation_id TEXT);
             CREATE TABLE library_members(library_id TEXT NOT NULL,user_id TEXT NOT NULL,PRIMARY KEY(library_id,user_id));
             CREATE TABLE recording_credit_observations(observation_id TEXT PRIMARY KEY,recording_id TEXT NOT NULL);
             CREATE INDEX recording_credit_observation_recording ON recording_credit_observations(recording_id);
             CREATE TABLE account_recording_credits(user_id TEXT NOT NULL,recording_id TEXT NOT NULL,artist_id TEXT NOT NULL,PRIMARY KEY(user_id,recording_id));
             CREATE TABLE recording_credited_artist_observations(observation_id TEXT NOT NULL,position INTEGER NOT NULL,recording_id TEXT NOT NULL,name TEXT NOT NULL,role TEXT NOT NULL,PRIMARY KEY(observation_id,position));
             CREATE INDEX recording_credited_artist_observation_recording ON recording_credited_artist_observations(recording_id);
             CREATE TABLE personal_recording_metadata(user_id TEXT NOT NULL,recording_id TEXT NOT NULL,override_artist TEXT);
             CREATE TABLE account_recording_credit_members(user_id TEXT NOT NULL,recording_id TEXT NOT NULL,artist_id TEXT NOT NULL,role TEXT NOT NULL,PRIMARY KEY(user_id,recording_id,artist_id));
             CREATE TABLE account_artists(user_id TEXT NOT NULL,id TEXT NOT NULL,library_id TEXT NOT NULL,name TEXT NOT NULL COLLATE BINARY,PRIMARY KEY(user_id,id),UNIQUE(user_id,library_id,name));
             CREATE TABLE artists(id TEXT NOT NULL,library_id TEXT NOT NULL,name TEXT NOT NULL COLLATE BINARY,PRIMARY KEY(id));
             CREATE UNIQUE INDEX artists_library_name ON artists(library_id,name COLLATE BINARY);
             INSERT INTO recordings VALUES('repair-recording','repair-library','repair-observation');
             INSERT INTO library_members VALUES('repair-library','repair-user');
             INSERT INTO recording_credit_observations VALUES('repair-observation','repair-recording');
             INSERT INTO account_recording_credits VALUES('repair-user','repair-recording','whole-credit');
             INSERT INTO artists VALUES('shared-ada','repair-library','Ada');
             INSERT INTO recording_credited_artist_observations VALUES
                ('repair-observation',0,'repair-recording','Ada','main'),
                ('repair-observation',1,'repair-recording','Cy','featured');",
        )
        .execute(&pool)
        .await
        .unwrap();
        if existing_main {
            sqlx::query("INSERT INTO account_artists VALUES('repair-user','shared-ada','repair-library','Ada')")
                .execute(&pool)
                .await
                .unwrap();
            sqlx::query("INSERT INTO account_recording_credit_members VALUES('repair-user','repair-recording','shared-ada','main')")
                .execute(&pool)
                .await
                .unwrap();
        }
        if history > 0 {
            sqlx::query(
                "WITH RECURSIVE sequence(value) AS (
                    SELECT 1 WHERE ? > 0 UNION ALL SELECT value+1 FROM sequence WHERE value < ?
                 )
                 INSERT INTO recording_credited_artist_observations(observation_id,position,recording_id,name,role)
                 SELECT 'unrelated-history-'||value,0,'repair-recording','History','main' FROM sequence",
            )
            .bind(history)
            .bind(history)
            .execute(&pool)
            .await
            .unwrap();
        }
        pool
    }

    async fn plans(pool: &SqlitePool) -> Vec<String> {
        sqlx::query(REPAIR_CANDIDATE_TABLE)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO member_repair_candidates VALUES('repair-user','repair-library','repair-recording','Ada')")
            .execute(pool)
            .await
            .unwrap();
        let mut plans = Vec::new();
        for statement in [
            REPAIR_CANDIDATES,
            REGISTER_REPAIR_ARTISTS,
            BIND_REPAIR_MEMBERS,
        ] {
            let explained = format!("EXPLAIN QUERY PLAN {statement}");
            let rows: Vec<(i64, i64, i64, String)> =
                sqlx::query_as(&explained).fetch_all(pool).await.unwrap();
            plans.extend(rows.into_iter().map(|row| row.3));
        }
        sqlx::query("DROP TABLE member_repair_candidates")
            .execute(pool)
            .await
            .unwrap();
        plans
    }

    async fn measured(history: i64, existing_main: bool) -> (u64, i64) {
        let pool = pool(history, existing_main).await;
        let meter = WorkMeter::install(&pool).await;
        let (result, steps) = meter.measure(repair_missing_main_members(&pool)).await;
        meter.remove(&pool).await;
        result.unwrap();
        let rows: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM account_recording_credit_members WHERE recording_id='repair-recording' AND role='main'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        pool.close().await;
        (steps, rows)
    }

    async fn measured_without_current_recording(history: i64) -> (u64, i64) {
        let pool = pool(history, false).await;
        sqlx::query("DELETE FROM recordings WHERE id='repair-recording'")
            .execute(&pool)
            .await
            .unwrap();
        let meter = WorkMeter::install(&pool).await;
        let (result, steps) = meter.measure(repair_missing_main_members(&pool)).await;
        meter.remove(&pool).await;
        result.unwrap();
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM account_recording_credit_members")
            .fetch_one(&pool)
            .await
            .unwrap();
        pool.close().await;
        (steps, rows)
    }

    #[tokio::test]
    async fn current_member_repair_sql_is_indexed_and_bounded_by_current_work() {
        let plan_pool = pool(8192, false).await;
        let plan_rows = plans(&plan_pool).await;
        eprintln!("repair_query_plan=\n{}", plan_rows.join("\n"));
        let plan = plan_rows.join("\n");
        assert!(
            plan.contains("SEARCH main"),
            "current main lookup must be indexed: {plan}"
        );
        assert!(
            plan.contains("main USING INDEX sqlite_autoindex_recording_credited_artist_observations_1 (observation_id=?"),
            "current main lookup must be keyed by the selected observation: {plan}"
        );
        assert!(
            plan.contains("SEARCH one_main"),
            "main count must probe the selected observation: {plan}"
        );
        assert!(
            plan.contains("one_main USING INDEX sqlite_autoindex_recording_credited_artist_observations_1 (observation_id=?"),
            "main count must be keyed by the selected observation: {plan}"
        );
        assert!(
            !plan.contains("SCAN recording_credited_artist_observations"),
            "repair SQL must not scan unrelated historical observations: {plan}"
        );
        assert!(
            !plan.contains("SCAN main"),
            "main projection must use an observation index: {plan}"
        );
        assert!(
            !plan.contains("SCAN one_main"),
            "main count must use an observation index: {plan}"
        );
        plan_pool.close().await;

        let (empty_small, empty_small_rows) = measured(0, true).await;
        let (empty_large, empty_large_rows) = measured(8192, true).await;
        eprintln!("repair_vm_steps_noop_history_0={empty_small} history_8192={empty_large}");
        assert_eq!((empty_small_rows, empty_large_rows), (1, 1));
        assert!(
            empty_large <= empty_small + 2 * VM_STEP_QUANTUM,
            "no-work startup must not scale with unrelated observation history: small={empty_small}, large={empty_large}"
        );

        let (repair_small, repair_small_rows) = measured(0, false).await;
        let (repair_large, repair_large_rows) = measured(8192, false).await;
        eprintln!(
            "repair_vm_steps_one_repair_history_0={repair_small} history_8192={repair_large}"
        );
        assert_eq!((repair_small_rows, repair_large_rows), (1, 1));
        assert!(
            repair_large <= repair_small + 2 * VM_STEP_QUANTUM,
            "one current repair must not scale with unrelated observation history: small={repair_small}, large={repair_large}"
        );

        let (no_record_small, no_record_small_rows) = measured_without_current_recording(0).await;
        let (no_record_large, no_record_large_rows) =
            measured_without_current_recording(8192).await;
        eprintln!(
            "repair_vm_steps_no_current_recording_history_0={no_record_small} history_8192={no_record_large}"
        );
        assert_eq!((no_record_small_rows, no_record_large_rows), (0, 0));
        assert!(
            no_record_large <= no_record_small + 2 * VM_STEP_QUANTUM,
            "no-current-recording startup must not scale with unrelated observation history: small={no_record_small}, large={no_record_large}"
        );
    }
}
