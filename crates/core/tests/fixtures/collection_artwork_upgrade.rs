use sha2::{Digest, Sha256};
use sqlx::{Connection, SqliteConnection, migrate::Migrator, sqlite::SqliteConnectOptions};
use std::{borrow::Cow, path::Path};

pub const READY_RECORDING: &str = "20000000-0000-4000-8000-000000000001";
pub const PENDING_RECORDING: &str = "20000000-0000-4000-8000-000000000002";
pub const READY_JOB: &str = "50000000-0000-4000-8000-000000000001";
pub const PENDING_JOB: &str = "50000000-0000-4000-8000-000000000002";
pub const OWNER_TOKEN: &str = "legacy-fixture-token";
pub const OTHER_TOKEN: &str = "owned-historical-artwork-token";
pub type Schema = Vec<(String, String, String, Option<String>)>;
type ForeignKey = (String, String, String, String, String, String);

pub fn migration_failure(error: sqlx::migrate::MigrateError) -> ! {
    let (version, execution) = match error {
        sqlx::migrate::MigrateError::ExecuteMigration(error, version) => (Some(version), error),
        sqlx::migrate::MigrateError::Execute(error) => (None, error),
        _ => panic!("Owned artwork fixture migration failed without SQLite execution details"),
    };
    let Some(database) = execution.as_database_error() else {
        panic!("Owned artwork fixture migration failed: version={version:?}, code=non_database");
    };
    let code = database.code().unwrap_or_default();
    let message = match database.message() {
        "FOREIGN KEY constraint failed" => "FOREIGN KEY constraint failed",
        "database table is locked" => "database table is locked",
        "database is locked" => "database is locked",
        "cannot commit transaction - SQL statements in progress" => {
            "cannot commit transaction - SQL statements in progress"
        }
        _ => "SQLite migration failed (nonstatic message redacted)",
    };
    panic!(
        "Owned artwork fixture migration failed: version={version:?}, sqlite_code={code}, sqlite_message={message}"
    );
}

pub async fn connect(path: &Path) -> SqliteConnection {
    SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(path)
            .foreign_keys(true),
    )
    .await
    .unwrap()
}

pub fn through(version: i64) -> Migrator {
    let all = sqlx::migrate!("./migrations");
    Migrator {
        migrations: Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= version)
                .cloned()
                .collect(),
        ),
        ..Migrator::DEFAULT
    }
}

pub async fn seed(connection: &mut SqliteConnection, url: &str, cover: &[u8]) {
    through(50).run(&mut *connection).await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT max(version) FROM _sqlx_migrations WHERE success=1")
            .fetch_one(&mut *connection)
            .await
            .unwrap(),
        50
    );
    sqlx::query(
        "UPDATE music_folders SET directory_id='legacy-music',bound=1 WHERE host_device='server'",
    )
    .execute(&mut *connection)
    .await
    .unwrap();
    sqlx::query("INSERT INTO plugin_services(id,name,base_url) VALUES ('artwork-upgrade-service','Owned historical artwork',?)")
        .bind(url).execute(&mut *connection).await.unwrap();
    sqlx::raw_sql(include_str!("collection_artwork_upgrade.sql"))
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO sessions(token_hash,user_id,expires_at) VALUES (?,'artwork-other',4102444800)",
    )
    .bind(format!("{:x}", Sha256::digest(OTHER_TOKEN.as_bytes())))
    .execute(&mut *connection)
    .await
    .unwrap();
    let owner_hash = format!("{:x}", Sha256::digest(OWNER_TOKEN.as_bytes()));
    let other_hash = format!("{:x}", Sha256::digest(OTHER_TOKEN.as_bytes()));
    for (
        rowid,
        id,
        user,
        recording,
        session,
        observation,
        binding,
        installation,
        status,
        attempts,
        reservation,
    ) in [
        (
            701i64,
            READY_JOB,
            "legacy-admin",
            READY_RECORDING,
            owner_hash,
            "artwork-ready-observation",
            "artwork-ready-binding",
            "artwork-ready-installation",
            "ready",
            1i64,
            0i64,
        ),
        (
            1907,
            PENDING_JOB,
            "artwork-other",
            PENDING_RECORDING,
            other_hash,
            "artwork-pending-observation",
            "artwork-pending-binding",
            "artwork-pending-installation",
            "queued",
            0,
            2_097_152,
        ),
    ] {
        sqlx::query("INSERT INTO artwork_fetches(rowid,id,user_id,recording_id,session_hash,observation_id,binding_id,binding_revision,installation_id,installation_revision,service_id,service_revision,status,attempts,reservation_bytes,next_attempt_at,created_at,updated_at)
            VALUES (?,?,?,?,?,?,?,1,?,1,'artwork-upgrade-service',1,?,?,?,unixepoch()-1,123456,123457)")
            .bind(rowid).bind(id).bind(user).bind(recording).bind(session).bind(observation).bind(binding)
            .bind(installation).bind(status).bind(attempts).bind(reservation).execute(&mut *connection).await.unwrap();
    }
    let hash = format!("{:x}", Sha256::digest(cover));
    sqlx::query("INSERT INTO artwork_assets(rowid,user_id,content_hash,content_type,width,height,bytes) VALUES (509,'legacy-admin',?,'image/png',1,1,?)")
        .bind(&hash).bind(cover).execute(&mut *connection).await.unwrap();
    sqlx::query("INSERT INTO recording_artwork(rowid,user_id,recording_id,observation_id,fetch_id,content_hash) VALUES (811,'legacy-admin',?,'artwork-ready-observation',?,?)")
        .bind(READY_RECORDING).bind(READY_JOB).bind(hash).execute(&mut *connection).await.unwrap();
    assert_integrity(connection).await;
}

pub async fn schema(connection: &mut SqliteConnection) -> Schema {
    sqlx::query_as("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name")
        .fetch_all(connection)
        .await
        .unwrap()
}

pub async fn assert_schema_preserved(
    connection: &mut SqliteConnection,
    before: &Schema,
    replaced: &[(&str, &str)],
) {
    let changed = [
        "artwork_fetches",
        "artwork_pending_recording",
        "artwork_fetch_identity",
        "current_artwork_fetch_authority",
        "recording_artwork_owner",
        "recording_artwork_update",
    ];
    let after = schema(connection).await;
    for object in before.iter().filter(|object| {
        !changed.contains(&object.1.as_str())
            && !replaced
                .iter()
                .any(|(kind, name)| *kind == object.0 && *name == object.1)
    }) {
        let mut expected = object.clone();
        if expected.0 == "table" && expected.1 == "source_resolutions" {
            expected.3 = expected.3.map(|ddl| {
                ddl.replace(
                    "'no_suitable_source'",
                    "'no_suitable_source','provider_unavailable'",
                )
            });
        }
        if expected.0 == "table" && expected.1 == "playlist_follows" {
            expected.3 = expected.3.map(|ddl| {
                ddl.replacen(
                    "error TEXT\n)",
                    "error TEXT\n, approved_capabilities TEXT, approved_credential TEXT)",
                    1,
                )
            });
        }
        if expected.0 == "table" && expected.1 == "sessions" {
            expected.3 = expected.3.map(|ddl| {
                ddl.replacen(
                    "expires_at INTEGER NOT NULL\n)",
                    "expires_at INTEGER NOT NULL\n, created_at INTEGER NOT NULL DEFAULT 0, last_seen_at INTEGER NOT NULL DEFAULT 0, client TEXT CHECK (client IS NULL OR length(CAST(client AS BLOB)) BETWEEN 1 AND 64))",
                    1,
                )
            });
        }
        if expected.1 == "recording_artwork" {
            expected.3 = expected.3.map(|ddl| {
                ddl.replacen(
                    "CREATE TABLE recording_artwork",
                    "CREATE TABLE \"recording_artwork\"",
                    1,
                )
                .replacen(
                    "REFERENCES artwork_fetches(id)",
                    "REFERENCES \"artwork_fetches\"(id)",
                    1,
                )
            });
        }
        assert!(
            after.contains(&expected),
            "the artwork upgrade must retain unrelated schema and every original index and lifecycle trigger: {}",
            expected.1
        );
    }
    for name in changed {
        assert!(
            after.iter().any(|object| object.1 == name),
            "the artwork upgrade must restore each replaced schema object"
        );
    }
    assert!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM artwork_fetches WHERE entity_kind!='recording' OR release_id IS NOT NULL OR playlist_id IS NOT NULL").fetch_one(connection).await.unwrap() == 0, "legacy jobs must retain recording identity without invented collection targets");
}

pub async fn foreign_keys(connection: &mut SqliteConnection, table: &str) -> Vec<ForeignKey> {
    sqlx::query_as("SELECT \"table\",\"from\",\"to\",on_update,on_delete,\"match\" FROM pragma_foreign_key_list(?) ORDER BY \"table\",\"from\",\"to\"")
        .bind(table).fetch_all(connection).await.unwrap()
}

pub async fn assert_integrity(connection: &mut SqliteConnection) {
    assert_eq!(
        sqlx::query_scalar::<_, i64>("PRAGMA foreign_keys")
            .fetch_one(&mut *connection)
            .await
            .unwrap(),
        1
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut *connection)
            .await
            .unwrap()
            .is_empty(),
        "the historical artwork fixture must have no dangling references"
    );
    assert!(
        sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_one(connection)
            .await
            .unwrap()
            == "ok"
    );
}
