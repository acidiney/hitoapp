#[path = "fixtures/maintenance.rs"]
mod fixture;

use axum::{Router, http::StatusCode};
use sqlx::Connection;
use std::{
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{process::Command, sync::oneshot};

#[tokio::test]
async fn migration_only_cli_upgrades_an_existing_library_without_starting_application_work() {
    let requests = Arc::new(AtomicUsize::new(0));
    let observed = requests.clone();
    let provider = Router::new().fallback(move || {
        let observed = observed.clone();
        async move {
            observed.fetch_add(1, Ordering::SeqCst);
            StatusCode::SERVICE_UNAVAILABLE
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/", listener.local_addr().unwrap());
    let (stop, stopped) = oneshot::channel();
    let serving = tokio::spawn(async move {
        axum::serve(listener, provider)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let library = fixture::Library::version_29(&endpoint).await;
    let before = fixture::Snapshot::read(&library.database).await;
    let files_before = library.files();
    let missing = library.directory.path().join("unavailable");
    let mut child = Command::new(env!("CARGO_BIN_EXE_hito-server"))
        .arg("migrate")
        .arg("--database")
        .arg(&library.database)
        .args(["--expect-version", "29"])
        .env("HITO_DATABASE", &library.database)
        .env("HITO_WEB_DIR", missing.join("web"))
        .env("HITO_MUSIC_ROOT", missing.join("music"))
        .env_remove("HITO_STORAGE_MANIFEST")
        .env("HITO_FFMPEG", missing.join("ffmpeg"))
        .env("HITO_FFPROBE", missing.join("ffprobe"))
        .env_remove("HITO_DEVICE_ID")
        .env("HITO_LISTEN", "127.0.0.1:0")
        .env("HITO_LOG_FORMAT", "json")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(20), child.wait()).await;
    if result.is_err() {
        child.kill().await.unwrap();
    }
    let _ = stop.send(());
    serving.await.unwrap();
    let status = result.expect("migration-only CLI must terminate").unwrap();
    assert!(
        status.success(),
        "migration-only CLI must succeed without web assets, music grants, media tools, or runtime configuration"
    );
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    before.assert_preserved(&library.database).await;
    assert!(
        library.files() == files_before,
        "maintenance must preserve all non-SQLite files and create no application sidecars"
    );

    let mut database = fixture::connect(&library.database).await;
    let applied: Vec<(i64, Vec<u8>, bool)> =
        sqlx::query_as("SELECT version,checksum,success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&mut database)
            .await
            .unwrap();
    let migrations = fixture::migrations();
    assert_eq!(applied.len(), migrations.iter().count());
    for (actual, expected) in applied.iter().zip(migrations.iter()) {
        assert_eq!(actual.0, expected.version);
        assert!(actual.1.as_slice() == expected.checksum.as_ref());
        assert!(actual.2);
    }
    assert!(applied.last().unwrap().0 > 29);
    let roots: Vec<(String, String, String, String)> =
        sqlx::query_as("SELECT id,transfer_id,kind,state FROM download_retentions ORDER BY id")
            .fetch_all(&mut database)
            .await
            .unwrap();
    assert_eq!(roots.len(), 4);
    for (index, (root, transfer, kind, state)) in roots.iter().enumerate() {
        assert_eq!(root, &fixture::id(100 + index as u128));
        assert_eq!(transfer, root);
        assert_eq!(kind, "recording");
        assert_eq!(state, if index == 3 { "removed" } else { "active" });
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM download_intents intent JOIN download_retentions root ON root.id=intent.id
            WHERE root.account_id IS NOT intent.account_id OR root.host_device IS NOT intent.host_device
                OR root.recording_id IS NOT intent.recording_id OR root.access_grant_id IS NOT intent.access_grant_id
                OR root.playlist_id IS NOT NULL OR root.created_at IS NOT intent.created_at OR root.updated_at IS NOT intent.updated_at",
        )
        .fetch_one(&mut database)
        .await
        .unwrap(),
        0,
        "recording retention backfill must preserve transfer ownership and original timestamps"
    );
    let failed: Vec<String> =
        sqlx::query_scalar("SELECT intent_id FROM download_terminal_failures ORDER BY sequence")
            .fetch_all(&mut database)
            .await
            .unwrap();
    assert_eq!(failed, vec![fixture::id(101)]);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM download_receipts WHERE retention_id IS NOT NULL",
        )
        .fetch_one(&mut database)
        .await
        .unwrap(),
        0,
        "the migration must preserve historical receipt rows and leave their new nullable column empty"
    );
    assert!(
        sqlx::query("PRAGMA foreign_key_check")
            .fetch_all(&mut database)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("PRAGMA integrity_check")
            .fetch_one(&mut database)
            .await
            .unwrap(),
        "ok"
    );
    database.close().await.unwrap();
}
