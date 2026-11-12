use crate::{BeginCatalogSnapshot, ConnectReplica, Credentials, Error, test_support::support};
use axum::{
    Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use serde_json::json;
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};
use uuid::Uuid;

#[tokio::test]
async fn a_failed_commit_retains_authority_and_admission_until_queued_rollback_finishes() {
    let (_directory, services, actor, _, _) = support::library("catalog-commit-settlement").await;
    let snapshot = Arc::new(Mutex::new(Uuid::new_v4()));
    let router=Router::new()
        .route("/api/v1/login",post(||async{Json(json!({"token":"owned-remote-session","user":{"id":"remote-account","username":"remote","admin":false,"display_name":null,"avatar_version":null}}))}))
        .route("/api/v1/me",get(||async{Json(json!({"id":"remote-account","username":"remote","admin":false,"display_name":null,"avatar_version":null}))}))
        .route("/api/v1/catalog/snapshots",post(|State(id):State<Arc<Mutex<Uuid>>>|async move {Json(json!({"id":*id.lock().unwrap(),"account_id":"remote-account","library_id":"library","mode":"snapshot_only","collection_watermark":0,"recording_count":1,"page_count":1,"expires_at":1}))}))
        .route("/api/v1/catalog/snapshots/{id}/pages/{ordinal}",get(|State(id):State<Arc<Mutex<Uuid>>>|async move {Json(json!({"snapshot_id":*id.lock().unwrap(),"ordinal":0,"next_ordinal":null,"recordings":[{"id":"recording","library_id":"library","title":"Track","artist":"Artist","duration_ms":0,"sources":[{"id":"source","observation_id":null,"title":"Track","artist":"Artist","duration_ms":0,"server_relay":"unavailable","release":null}]}]}))}))
        .route("/api/v1/catalog/snapshots/{id}",axum::routing::delete(||async {StatusCode::NO_CONTENT}))
        .with_state(snapshot.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let serving = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    let connection = services
        .replicas
        .connect(
            &actor,
            ConnectReplica {
                endpoint,
                credentials: Credentials {
                    username: "remote".into(),
                    password: "owned password".into(),
                },
            },
        )
        .await
        .unwrap();
    let session = services
        .replicas
        .activate(&actor, connection.id)
        .await
        .unwrap();
    let first = services
        .replicas
        .install_catalog(
            &actor,
            &session,
            BeginCatalogSnapshot {
                library_id: "library".into(),
            },
        )
        .await
        .unwrap();
    let profile = services.replicas.catalog_profile(&session).unwrap();
    sqlx::raw_sql("CREATE TABLE owned_commit_fault (principal TEXT REFERENCES users(id) DEFERRABLE INITIALLY DEFERRED); CREATE TRIGGER owned_head_commit_fault AFTER UPDATE ON replica_catalog_heads BEGIN INSERT INTO owned_commit_fault VALUES('absent-principal'); END;").execute(&profile.store.pool).await.unwrap();
    let entered = Arc::new(tokio::sync::Notify::new());
    let stopped = Arc::new(AtomicBool::new(false));
    let (release, waiting) = mpsc::channel();
    let waiting = Arc::new(Mutex::new(waiting));
    let mut connections = Vec::new();
    for _ in 0..5 {
        let mut connection = profile.store.pool.acquire().await.unwrap();
        let entered = entered.clone();
        let stopped = stopped.clone();
        let waiting = waiting.clone();
        connection
            .lock_handle()
            .await
            .unwrap()
            .set_rollback_hook(move || {
                if !stopped.swap(true, Ordering::SeqCst) {
                    entered.notify_one();
                    let _ = waiting.lock().unwrap().recv_timeout(Duration::from_secs(5));
                }
            });
        connections.push(connection);
    }
    drop(connections);
    *snapshot.lock().unwrap() = Uuid::new_v4();
    let replicas = services.replicas.clone();
    let owner = actor.clone();
    let lease = session.clone();
    let caller = tokio::spawn(async move {
        replicas
            .install_catalog(
                &owner,
                &lease,
                BeginCatalogSnapshot {
                    library_id: "library".into(),
                },
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), entered.notified())
        .await
        .unwrap();
    caller.abort();
    assert!(caller.await.unwrap_err().is_cancelled());
    assert_eq!(services.replicas.profiles.catalog.lock().unwrap().len(), 1);
    let replicas = services.replicas.clone();
    let owner = actor.clone();
    let lease = session.clone();
    assert!(matches!(
        replicas.synchronize(&owner, &lease).await,
        Err(Error::Conflict(_))
    ));
    let identity = services.identity.clone();
    let owner = actor.clone();
    let control = tokio::spawn(async move {
        identity
            .set_display_name(&owner, Some("Control after settlement".into()))
            .await
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        !control.is_finished(),
        "control guard was dropped before the SQLite rollback worker settled"
    );
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), control)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while !services
            .replicas
            .profiles
            .catalog
            .lock()
            .unwrap()
            .is_empty()
        {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        services
            .replicas
            .catalog_snapshot(&actor, &session, "library")
            .await
            .unwrap()
            .unwrap()
            .id,
        first.id
    );
    services.replicas.close().await.unwrap();
    serving.abort();
    let _ = serving.await;
}
