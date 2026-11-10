use super::Vault;
use crate::{CoreConfig, Credentials, HostDeviceId};
use std::path::Path;

const TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[tokio::test]
async fn replica_credentials_keep_their_key_and_binding_through_loss_and_restore() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("hito.db");
    let config = || {
        CoreConfig::new(
            database.clone(),
            directory.path().join("music"),
            HostDeviceId::new("owned-vault-replica").unwrap(),
        )
    };
    let core = crate::open(config()).await.unwrap();
    let owner = core
        .identity
        .setup(Credentials {
            username: "owner".into(),
            password: "owned-private-password".into(),
        })
        .await
        .unwrap();
    let vault = Vault::open(&core.store, &database).await.unwrap();
    let sealed = vault
        .seal_replica(
            &owner.user.id,
            "binding",
            "http://private-server.invalid/",
            "remote-account",
            TOKEN,
        )
        .unwrap();
    assert_eq!(
        vault
            .reveal_replica(
                &owner.user.id,
                "binding",
                "http://private-server.invalid/",
                "remote-account",
                &sealed
            )
            .unwrap()
            .as_str(),
        TOKEN
    );
    for (account, binding, endpoint, remote) in [
        (
            "another-owner",
            "binding",
            "http://private-server.invalid/",
            "remote-account",
        ),
        (
            owner.user.id.as_str(),
            "another-binding",
            "http://private-server.invalid/",
            "remote-account",
        ),
        (
            owner.user.id.as_str(),
            "binding",
            "https://private-server.invalid/",
            "remote-account",
        ),
        (
            owner.user.id.as_str(),
            "binding",
            "http://private-server.invalid/",
            "another-remote",
        ),
    ] {
        assert!(
            vault
                .reveal_replica(account, binding, endpoint, remote, &sealed)
                .is_err()
        );
    }
    assert!(
        vault
            .reveal(
                &owner.user.id,
                "binding",
                "http://private-server.invalid/",
                &sealed
            )
            .is_err()
    );
    assert!(
        !sealed
            .windows(TOKEN.len())
            .any(|part| part == TOKEN.as_bytes())
    );
    sqlx::query("INSERT INTO replica_connections(id,user_id,endpoint,remote_account_id,remote_user,client_id,state,credential) VALUES ('binding',?,'http://private-server.invalid/','remote-account','{}','replica-client','ready',?)")
        .bind(&owner.user.id).bind(&sealed).execute(&core.store.pool).await.unwrap();
    let key_path = database.with_extension("plugin-key");
    let key = std::fs::read(&key_path).unwrap();
    core.store.pool.close().await;
    drop(core);
    drop(vault);
    std::fs::remove_file(&key_path).unwrap();
    let missing = crate::open(config()).await.unwrap();
    assert!(missing.healthy().await);
    assert!(
        !key_path.exists(),
        "Replica-only encrypted credentials must prevent replacement of the missing key"
    );
    assert!(
        !Vault::open(&missing.store, &database)
            .await
            .unwrap()
            .ready()
    );
    let retained: Vec<u8> =
        sqlx::query_scalar("SELECT credential FROM replica_connections WHERE id='binding'")
            .fetch_one(&missing.store.pool)
            .await
            .unwrap();
    assert_eq!(retained, sealed);
    missing.store.pool.close().await;
    drop(missing);
    restore(&key_path, &[0x77; 32]);
    let mismatched = crate::open(config()).await.unwrap();
    assert!(mismatched.healthy().await);
    assert!(
        !Vault::open(&mismatched.store, &database)
            .await
            .unwrap()
            .ready()
    );
    assert_eq!(
        std::fs::read(&key_path).unwrap(),
        vec![0x77; 32],
        "Mismatched keys must not be silently replaced"
    );
    let still_retained: Vec<u8> =
        sqlx::query_scalar("SELECT credential FROM replica_connections WHERE id='binding'")
            .fetch_one(&mismatched.store.pool)
            .await
            .unwrap();
    assert_eq!(still_retained, sealed);
    mismatched.store.pool.close().await;
    drop(mismatched);
    restore(&key_path, &key);
    let restored = crate::open(config()).await.unwrap();
    let vault = Vault::open(&restored.store, &database).await.unwrap();
    assert_eq!(
        vault
            .reveal_replica(
                &owner.user.id,
                "binding",
                "http://private-server.invalid/",
                "remote-account",
                &retained
            )
            .unwrap()
            .as_str(),
        TOKEN
    );
    restored.store.pool.close().await;
}

fn restore(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
}

#[tokio::test]
async fn replica_contexts_cannot_be_rearranged_or_corrupted_and_tokens_stay_bounded() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("hito.db");
    let core = crate::open(CoreConfig::new(
        database.clone(),
        directory.path().join("music"),
        HostDeviceId::new("owned-replica-context").unwrap(),
    ))
    .await
    .unwrap();
    let vault = Vault::open(&core.store, &database).await.unwrap();
    let sealed = vault
        .seal_replica(
            "one|two",
            "three",
            "http://private.invalid/",
            "remote",
            TOKEN,
        )
        .unwrap();
    assert!(
        vault
            .reveal_replica(
                "one",
                "two|three",
                "http://private.invalid/",
                "remote",
                &sealed
            )
            .is_err()
    );
    let mut corrupt = sealed.clone();
    *corrupt.last_mut().unwrap() ^= 1;
    for envelope in [corrupt, vec![], sealed[..33].to_vec(), vec![0; 4170]] {
        assert!(
            vault
                .reveal_replica(
                    "one|two",
                    "three",
                    "http://private.invalid/",
                    "remote",
                    &envelope
                )
                .is_err()
        );
    }
    for invalid in [
        String::new(),
        "two words".into(),
        "private\r\nvalue".into(),
        "a".repeat(4097),
    ] {
        assert!(
            vault
                .seal_replica(
                    "owner",
                    "binding",
                    "http://private.invalid/",
                    "remote",
                    &invalid
                )
                .is_err()
        );
    }
    core.store.pool.close().await;
}

#[tokio::test]
async fn replica_fingerprint_discovery_has_bounded_work_as_credentials_grow() {
    use sqlx::{
        QueryBuilder, Sqlite,
        sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    };
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("hito.db");
    let core = crate::open(CoreConfig::new(
        database.clone(),
        directory.path().join("music"),
        HostDeviceId::new("owned-replica-startup-work").unwrap(),
    ))
    .await
    .unwrap();
    let owner = core
        .identity
        .setup(Credentials {
            username: "owner".into(),
            password: "owned-private-password".into(),
        })
        .await
        .unwrap();
    let vault = Vault::open(&core.store, &database).await.unwrap();
    for first in (0..16_384).step_by(128) {
        let rows = (first..first + 128)
            .map(|index| {
                let binding = format!("binding-{index}");
                let remote = format!("remote-{index}");
                let credential = vault
                    .seal_replica(
                        &owner.user.id,
                        &binding,
                        "http://private.invalid/",
                        &remote,
                        TOKEN,
                    )
                    .unwrap();
                (binding, remote, credential)
            })
            .collect::<Vec<_>>();
        let mut insert = QueryBuilder::<Sqlite>::new(
            "INSERT INTO replica_connections(id,user_id,endpoint,remote_account_id,remote_user,client_id,state,credential) ",
        );
        insert.push_values(rows, |mut row, (binding, remote, credential)| {
            row.push_bind(binding)
                .push_bind(&owner.user.id)
                .push_bind("http://private.invalid/")
                .push_bind(remote)
                .push_bind("{}")
                .push_bind("replica-client")
                .push_bind("ready")
                .push_bind(credential);
        });
        insert.build().execute(&core.store.pool).await.unwrap();
    }
    core.store.pool.close().await;
    for analyzed in [false, true] {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(&database)
                    .foreign_keys(true),
            )
            .await
            .unwrap();
        if analyzed {
            sqlx::query("ANALYZE").execute(&pool).await.unwrap();
        }
        let steps = Arc::new(AtomicU64::new(0));
        let counter = steps.clone();
        let mut connection = pool.acquire().await.unwrap();
        sqlx::query("SELECT id FROM users WHERE 0")
            .execute(&mut *connection)
            .await
            .unwrap();
        connection
            .lock_handle()
            .await
            .unwrap()
            .set_progress_handler(100, move || {
                counter.fetch_add(100, Ordering::Relaxed);
                true
            });
        drop(connection);
        let store = crate::store::Store { pool };
        assert!(Vault::open(&store, &database).await.unwrap().ready());
        let work = steps.load(Ordering::Relaxed);
        let mut connection = store.pool.acquire().await.unwrap();
        connection
            .lock_handle()
            .await
            .unwrap()
            .remove_progress_handler();
        drop(connection);
        assert!(
            work <= 2_000,
            "Replica credential startup scanned history: analyzed={analyzed}, vm_steps={work}"
        );
        eprintln!(
            "Replica fingerprint startup: credentials=16384 analyzed={analyzed} vm_steps={work}"
        );
        store.pool.close().await;
    }
}
