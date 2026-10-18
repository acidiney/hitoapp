#[allow(dead_code)]
#[path = "fixtures/manual_choice_http.rs"]
mod choice_http;
#[path = "fixtures/manual_choice_collection.rs"]
mod collection;
#[allow(dead_code)]
#[path = "fixtures/source_resolution_disconnect.rs"]
mod fixture;
#[allow(dead_code)]
#[path = "fixtures/download_host.rs"]
mod host;
#[allow(dead_code)]
#[path = "fixtures/artwork_http.rs"]
mod http;
#[path = "fixtures/source_resolution_publication.rs"]
mod publication;

use axum::http::StatusCode;
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::{future::Future, net::SocketAddr, panic::AssertUnwindSafe, path::Path, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

#[tokio::test]
async fn disconnected_http_publication_commits_before_its_host_stops() {
    run(false).await;
}

#[tokio::test]
async fn forced_connection_abort_keeps_publication_owned_until_its_host_stops() {
    run(true).await;
}

async fn run(retain_client: bool) {
    let directory = tempfile::tempdir().unwrap();
    let services = host::open(directory.path(), "publication-shutdown-host").await;
    let signals = publication::Signals::default();
    let mut server = publication::Host::start(services, signals.clone()).await;
    let mut provider = fixture::Provider::start().await;
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(directory.path().join("hito.db"))
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let result = tokio::time::timeout(
        Duration::from_secs(15),
        AssertUnwindSafe(exercise(
            directory.path(),
            &mut server,
            &provider,
            &pool,
            &signals,
            retain_client,
        ))
        .catch_unwind(),
    )
    .await;
    provider.release_search();
    pool.close().await;
    server.request_shutdown();
    let joined = server.join().await;
    let provider_joined = provider.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the owned HTTP publication workflow must finish within its deadline"),
    }
    assert!(
        joined && provider_joined,
        "all owned servers must stop and join"
    );
}

async fn exercise(
    directory: &Path,
    server: &mut publication::Host,
    provider: &fixture::Provider,
    pool: &sqlx::SqlitePool,
    signals: &publication::Signals,
    retain_client: bool,
) {
    let address = server.address;
    let session = post(
        "setup",
        address,
        "/api/v1/setup",
        json!({"username": "owner", "password": "owned HTTP publication password"}),
        None,
        StatusCode::CREATED,
    )
    .await;
    let token = session["token"].as_str().unwrap();
    let service = post(
        "offer_service",
        address,
        "/api/v1/plugins/services",
        json!({"name": "Owned publication service", "base_url": provider.url}),
        Some(token),
        StatusCode::CREATED,
    )
    .await;
    let installed = post("install", address, "/api/v1/plugins", json!({"service_id": service["id"], "service_revision": service["revision"], "bearer_token": null}), Some(token), StatusCode::CREATED).await;
    let original = post("adopt", address, &format!("/api/v1/plugins/{}/recordings", installed["id"].as_str().unwrap()), json!({"installation_revision": installed["revision"], "provider_recording_id": fixture::WAITING}), Some(token), StatusCode::OK).await;
    assert_eq!(original["available"], false);
    let recording = original["id"].as_str().unwrap();
    let snapshot = at_phase(
        "prepare_collection",
        collection::CollectionSnapshot::prepare(address, token, recording),
    )
    .await;
    at_phase(
        "initial_collection",
        collection::assert_collection(address, token, recording, &snapshot, false),
    )
    .await;
    let resolve = format!("/api/v1/recordings/{recording}/sources/resolve");
    let request = json!({"request_id": "2c9f4b22-a9b5-4dc1-8ae8-dcf23ee70b36"});
    let mut client = Some(begin(address, token, &resolve, &request).await);
    tokio::time::timeout(Duration::from_secs(3), provider.search_entered())
        .await
        .expect("real HTTP admission must commit before the provider search is held");
    assert_eq!(provider.counts(), ((0, 0, 0), 1, 0));
    let writer = tokio::time::timeout(Duration::from_secs(3), pool.begin_with("BEGIN IMMEDIATE"))
        .await
        .expect("the owned external writer must acquire SQLite after admission")
        .unwrap();
    provider.release_search();
    let resolution = signals.writing().await;
    assert_eq!(provider.counts(), ((0, 0, 0), 1, 2));
    if !retain_client {
        drop(client.take());
    }
    server.request_shutdown();
    signals.shutdown().await;
    if retain_client {
        signals.forced().await;
    }
    signals.assert_draining_before_release().await;
    assert!(
        !server.is_finished(),
        "the host task must remain pending while accepted publication is blocked"
    );
    if let Some(mut client) = client.take() {
        let mut response = Vec::new();
        tokio::time::timeout(Duration::from_secs(1), client.read_to_end(&mut response))
            .await
            .expect("the five-second connection abort must close the retained real TCP client")
            .unwrap();
        assert!(
            response.is_empty(),
            "publication must have no HTTP acknowledgement before commit"
        );
    }
    writer.rollback().await.unwrap();
    assert!(
        server.join().await,
        "the released publication must settle and its original host must join"
    );
    signals.assert_committed_and_stopped();
    assert_eq!(provider.counts(), ((0, 0, 0), 1, 2));

    let services = host::open(directory, "publication-shutdown-host").await;
    *server = publication::Host::start(services, publication::Signals::default()).await;
    let address = server.address;
    let sources = at_phase(
        "sources_after_restart",
        http::get(
            address,
            token,
            &format!("/api/v1/recordings/{recording}/sources"),
        ),
    )
    .await;
    assert_eq!(sources.as_array().unwrap().len(), 1);
    assert_eq!(sources[0]["recording_id"], recording);
    assert_eq!(sources[0]["ready"], true);
    let replay = post(
        "receipt_after_restart",
        address,
        &resolve,
        request.clone(),
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert_eq!(replay["request_id"], request["request_id"]);
    assert_eq!(replay["resolution_id"], resolution.to_string());
    assert_eq!(replay["recording_id"], recording);
    assert_eq!(replay["outcome"], "ready");
    assert!(
        replay["sources"] == sources,
        "replayed publication must retain its committed source identities"
    );
    let repeated = post(
        "exact_receipt_replay",
        address,
        &resolve,
        request,
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert!(
        repeated == replay,
        "the exact committed receipt must replay unchanged after restart"
    );
    at_phase(
        "collection_after_restart",
        collection::assert_collection(address, token, recording, &snapshot, true),
    )
    .await;
    let download = at_phase(
        "download_absence",
        http::call(
            address,
            "GET",
            &format!("/api/v1/recordings/{recording}/download"),
            None,
            Some(token),
        ),
    )
    .await;
    assert_eq!(
        download.status,
        StatusCode::NOT_FOUND,
        "automatic publication must not create a download intent"
    );
    assert_eq!(
        provider.counts(),
        ((0, 0, 0), 1, 2),
        "restart and exact replay must not repeat search, hydration or audio requests"
    );
    let encoded = replay.to_string();
    for private in [
        fixture::TARGET,
        fixture::CHOSEN,
        fixture::WAITING,
        fixture::WAITING_CANDIDATE,
        provider.url.as_str(),
    ] {
        assert!(
            !encoded.contains(private),
            "receipt must omit provider identities and URLs"
        );
    }
}

async fn begin(address: SocketAddr, token: &str, path: &str, body: &Value) -> TcpStream {
    tokio::time::timeout(Duration::from_secs(3), async {
        let body = body.to_string();
        let mut client = TcpStream::connect(address).await.unwrap();
        client.write_all(format!("POST {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Type: application/json\r\nX-Hito-Request: 1\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        client
    }).await.expect("the owned real TCP publication request must be sent")
}

async fn post(
    phase: &'static str,
    address: SocketAddr,
    path: &str,
    body: Value,
    token: Option<&str>,
    status: StatusCode,
) -> Value {
    at_phase(
        phase,
        choice_http::call(address, "POST", path, body, token, status),
    )
    .await
}

async fn at_phase<T>(phase: &'static str, pending: impl Future<Output = T>) -> T {
    AssertUnwindSafe(pending)
        .catch_unwind()
        .await
        .unwrap_or_else(|_| panic!("owned HTTP phase failed: {phase}"))
}
