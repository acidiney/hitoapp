use super::Vault;
use crate::{
    Action, Actor, CoreConfig, Credentials, HostDeviceId, Operation, Services, SyncBatch,
    store::Store,
};
use serde_json::{Value, json};
use sqlx::{
    QueryBuilder, Sqlite,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};
use uuid::Uuid;

const SMALL: u64 = 100_000;
const LARGE: u64 = 200_000;
const WORK_BUDGET: u64 = 10_000;
const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[tokio::test]
async fn retained_reporter_history_keeps_actual_vault_startup_bounded_without_weakening_key_recovery()
 {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("hito.db");
    let configuration = || {
        CoreConfig::new(
            database.clone(),
            directory.path().join("music"),
            HostDeviceId::new("owned-community-startup-work").unwrap(),
        )
    };
    let hito = crate::open(configuration()).await.unwrap();
    let owner = hito.identity.setup(credentials("owner")).await.unwrap();
    let owner = hito.identity.authenticate(&owner.token).await.unwrap();
    hito.identity
        .create_user(&owner, credentials("peer"))
        .await
        .unwrap();
    let peer = hito.identity.login(credentials("peer")).await.unwrap();
    let peer = hito.identity.authenticate(&peer.token).await.unwrap();
    for actor in [&owner, &peer] {
        hito.collection
            .apply(
                actor,
                SyncBatch {
                    device_id: "owned-startup-work".into(),
                    operations: vec![Operation {
                        id: Uuid::new_v4(),
                        action: Action::PlaylistCreate {
                            playlist_id: Uuid::new_v4(),
                            name: "private-retained-startup-playlist".into(),
                        },
                    }],
                },
            )
            .await
            .unwrap();
    }
    let before = [snapshot(&hito, &owner).await, snapshot(&hito, &peer).await];
    let store = connected_store(&database).await;
    let vault = Vault::open(&store, &database).await.unwrap();
    assert!(vault.ready());
    let key_path = database.with_extension("plugin-key");
    let key = std::fs::read(&key_path).unwrap();
    seed(&store, &vault, [&owner, &peer], 0, SMALL).await;
    let absent: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sqlite_schema WHERE name='sqlite_stat1'")
            .fetch_one(&store.pool)
            .await
            .unwrap();
    assert_eq!(
        absent, 0,
        "The absent case must have no inherited statistics"
    );
    let mut work = vec![("absent", measured_startup(&database, SMALL).await)];
    sqlx::query("ANALYZE").execute(&store.pool).await.unwrap();
    assert_statistics(&store, SMALL).await;
    seed(&store, &vault, [&owner, &peer], SMALL, LARGE).await;
    assert_statistics(&store, SMALL).await;
    work.push(("inherited", measured_startup(&database, LARGE).await));
    sqlx::query("ANALYZE").execute(&store.pool).await.unwrap();
    assert_statistics(&store, LARGE).await;
    work.push(("fresh", measured_startup(&database, LARGE).await));
    for actor in [&owner, &peer] {
        let (service, reporter, credential): (String, String, Vec<u8>) = sqlx::query_as(
            "SELECT service_key,reporter_id,credential FROM community_reporters WHERE user_id=? ORDER BY service_key DESC LIMIT 1",
        ).bind(&actor.user().id).fetch_one(&store.pool).await.unwrap();
        assert!(
            vault
                .reveal_community(&actor.user().id, &service, &reporter, &credential)
                .is_ok_and(|token| token.as_str() == TOKEN),
            "Retained reporters must use valid production ciphertext"
        );
    }
    assert!(std::fs::read(&key_path).unwrap() == key);
    store.pool.close().await;
    drop(vault);
    drop(hito);
    std::fs::remove_file(&key_path).unwrap();
    let unavailable = crate::open(configuration()).await.unwrap();
    assert!(unavailable.healthy().await);
    assert!(
        !key_path.exists(),
        "Retained reporter credentials must prevent silent key replacement"
    );
    for (actor, expected) in [&owner, &peer].into_iter().zip(&before) {
        assert!(
            !unavailable
                .plugins
                .settings(actor)
                .await
                .unwrap()
                .credential_storage_ready
        );
        assert!(
            snapshot(&unavailable, actor).await == *expected,
            "Key loss must preserve private account state"
        );
    }
    drop(unavailable);
    restore_key(&key_path, &key);
    let restored = crate::open(configuration()).await.unwrap();
    for (actor, expected) in [&owner, &peer].into_iter().zip(&before) {
        assert!(
            restored
                .plugins
                .settings(actor)
                .await
                .unwrap()
                .credential_storage_ready
        );
        assert!(
            snapshot(&restored, actor).await == *expected,
            "Startup must preserve private account state"
        );
    }
    let reporters: i64 = sqlx::query_scalar("SELECT count(*) FROM community_reporters")
        .fetch_one(&restored.store.pool)
        .await
        .unwrap();
    assert_eq!(reporters as u64, LARGE);
    assert!(
        work.iter().all(|(_, steps)| *steps <= WORK_BUDGET),
        "Actual Vault startup must keep fingerprint lookup work independent of retained reporter count: {work:?}"
    );
}

async fn measured_startup(database: &Path, rows: u64) -> u64 {
    let store = connected_store(database).await;
    let steps = Arc::new(AtomicU64::new(0));
    let counter = Arc::clone(&steps);
    let mut connection = store.pool.acquire().await.unwrap();
    connection
        .lock_handle()
        .await
        .unwrap()
        .set_progress_handler(100, move || {
            counter.fetch_add(100, Ordering::Relaxed);
            true
        });
    drop(connection);
    let started = Instant::now();
    let opened = Vault::open(&store, database).await;
    let elapsed = started.elapsed();
    let mut connection = store.pool.acquire().await.unwrap();
    connection
        .lock_handle()
        .await
        .unwrap()
        .remove_progress_handler();
    drop(connection);
    store.pool.close().await;
    let work = steps.load(Ordering::Relaxed);
    assert!(
        opened.unwrap().ready(),
        "Same-key reporter history must remain usable"
    );
    let (initialization, warm) = initialization_work(database).await;
    assert!(
        work.abs_diff(initialization + warm) <= 500,
        "Fresh Vault work must be explained by independently measured schema/statistics loading and the actual warmed Vault: full={work}, initialization={initialization}, warm={warm}"
    );
    eprintln!(
        "Actual Vault startup: retained_reporters={rows}, vm_steps={work}, schema_statistics_steps={initialization}, warm_vault_steps={warm}, elapsed={elapsed:?}"
    );
    work
}

async fn initialization_work(database: &Path) -> (u64, u64) {
    let store = connected_store(database).await;
    let steps = Arc::new(AtomicU64::new(0));
    let counter = Arc::clone(&steps);
    let mut connection = store.pool.acquire().await.unwrap();
    connection
        .lock_handle()
        .await
        .unwrap()
        .set_progress_handler(100, move || {
            counter.fetch_add(100, Ordering::Relaxed);
            true
        });
    sqlx::query("SELECT id FROM users WHERE 0")
        .fetch_all(&mut *connection)
        .await
        .unwrap();
    let initialization = steps.swap(0, Ordering::Relaxed);
    drop(connection);
    let opened = Vault::open(&store, database).await.unwrap();
    let warm = steps.load(Ordering::Relaxed);
    let mut connection = store.pool.acquire().await.unwrap();
    connection
        .lock_handle()
        .await
        .unwrap()
        .remove_progress_handler();
    drop(connection);
    store.pool.close().await;
    assert!(opened.ready());
    (initialization, warm)
}

async fn connected_store(database: &Path) -> Store {
    Store {
        pool: SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(database)
                    .foreign_keys(true),
            )
            .await
            .unwrap(),
    }
}

async fn seed(store: &Store, vault: &Vault, actors: [&Actor; 2], first: u64, end: u64) {
    let mut transaction = store.pool.begin().await.unwrap();
    for start in (first..end).step_by(128) {
        let rows = (start..(start + 128).min(end))
            .map(|index| {
                let account = actors[(index % 2) as usize].user().id.clone();
                let service = format!("{index:064x}");
                let reporter = Uuid::new_v4().to_string();
                let credential = vault
                    .seal_community(&account, &service, &reporter, TOKEN)
                    .unwrap();
                (account, service, reporter, credential)
            })
            .collect::<Vec<_>>();
        let mut query = QueryBuilder::<Sqlite>::new(
            "INSERT INTO community_reporters(user_id,service_key,reporter_id,credential) ",
        );
        query.push_values(rows, |mut row, (account, service, reporter, credential)| {
            row.push_bind(account)
                .push_bind(service)
                .push_bind(reporter)
                .push_bind(credential);
        });
        query.build().execute(&mut *transaction).await.unwrap();
    }
    transaction.commit().await.unwrap();
}

async fn assert_statistics(store: &Store, rows: u64) {
    let statistics: String = sqlx::query_scalar(
        "SELECT stat FROM sqlite_stat1 WHERE idx='community_credential_fingerprints'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap();
    assert_eq!(
        statistics
            .split_whitespace()
            .next()
            .unwrap()
            .parse::<u64>()
            .unwrap(),
        rows
    );
}

async fn snapshot(hito: &Services, actor: &Actor) -> Value {
    json!({
        "preferences": hito.community.preferences(actor).await.unwrap(),
        "playlists": hito.collection.playlists(actor).await.unwrap(),
        "activity": hito.collection.activity(actor).await.unwrap(),
        "events": hito.collection.events(actor, 0).await.unwrap(),
    })
}

fn credentials(username: &str) -> Credentials {
    Credentials {
        username: username.into(),
        password: "owned startup work fixture password".into(),
    }
}

fn restore_key(path: &Path, key: &[u8]) {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    options.open(path).unwrap().write_all(key).unwrap();
}
