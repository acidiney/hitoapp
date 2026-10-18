#[allow(dead_code)]
#[path = "fixtures/source_resolution_disconnect.rs"]
mod fixture;
#[path = "fixtures/source_resolution_handoff.rs"]
mod handoff;
#[allow(dead_code)]
#[path = "fixtures/download_host.rs"]
mod host;
#[allow(dead_code)]
#[path = "fixtures/artwork_http.rs"]
mod http;

use axum::http::StatusCode;
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::{
    collections::HashSet, future::Future, net::SocketAddr, panic::AssertUnwindSafe, time::Duration,
};
use tokio::{
    io::AsyncWriteExt,
    net::TcpStream,
    task::{JoinHandle, JoinSet},
};
use uuid::Uuid;

#[tokio::test]
async fn cancelled_http_cleanup_admits_one_successor_and_rejects_surplus_retries_before_sql() {
    let directory = tempfile::tempdir().unwrap();
    let services = host::open(directory.path(), "resolution-handoff-host").await;
    let entering = handoff::Handoff::default();
    let mut server = handoff::Host::start(services, entering.clone()).await;
    let mut provider = fixture::Provider::start().await;
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(directory.path().join("hito.db"))
            .foreign_keys(true),
    )
    .await
    .unwrap();
    let mut foreground = None;
    let mut surplus = JoinSet::new();
    let result = tokio::time::timeout(
        Duration::from_secs(15),
        AssertUnwindSafe(exercise(
            server.address,
            &provider,
            &pool,
            &entering,
            &mut foreground,
            &mut surplus,
        ))
        .catch_unwind(),
    )
    .await;
    surplus.abort_all();
    while surplus.join_next().await.is_some() {}
    if let Some(foreground) = foreground {
        foreground.abort();
        let _ = foreground.await;
    }
    provider.release_search();
    pool.close().await;
    server.request_shutdown();
    let joined = server.join().await;
    let provider_joined = provider.finish().await;
    match result {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the owned cancellation handoff must finish within its deadline"),
    }
    assert!(
        joined && provider_joined,
        "all owned protocol servers must finish"
    );
}

async fn exercise(
    address: SocketAddr,
    provider: &fixture::Provider,
    pool: &sqlx::SqlitePool,
    entering: &handoff::Handoff,
    foreground: &mut Option<JoinHandle<http::Response>>,
    surplus: &mut JoinSet<http::Response>,
) {
    let session = post(
        "setup",
        address,
        "/api/v1/setup",
        json!({"username": "owner", "password": "owned cancellation handoff password"}),
        None,
        StatusCode::CREATED,
    )
    .await;
    let token = session["token"].as_str().unwrap();
    let service = post(
        "offer_service",
        address,
        "/api/v1/plugins/services",
        json!({"name": "Owned handoff service", "base_url": provider.url}),
        Some(token),
        StatusCode::CREATED,
    )
    .await;
    let installed = post("install", address, "/api/v1/plugins", json!({"service_id": service["id"], "service_revision": service["revision"], "bearer_token": null}), Some(token), StatusCode::CREATED).await;
    let adoption = format!(
        "/api/v1/plugins/{}/recordings",
        installed["id"].as_str().unwrap()
    );
    let mut recordings = Vec::new();
    let mut metadata = Vec::new();
    for provider_recording in [fixture::TARGET, fixture::WAITING] {
        let original = post("adopt", address, &adoption, json!({"installation_revision": installed["revision"], "provider_recording_id": provider_recording}), Some(token), StatusCode::OK).await;
        assert_eq!(original["available"], false);
        let recording = original["id"].as_str().unwrap().to_owned();
        metadata.push(
            at_phase(
                "metadata_before",
                http::get(
                    address,
                    token,
                    &format!("/api/v1/recordings/{recording}/metadata"),
                ),
            )
            .await,
        );
        recordings.push(recording);
    }
    let foreground_path = format!("/api/v1/recordings/{}/sources/resolve", recordings[0]);
    let cancelled_path = format!("/api/v1/recordings/{}/sources/resolve", recordings[1]);
    let cancelled_request = json!({"request_id": "dc5405c2-b982-4248-a220-a510f590c35b"});
    let client = begin(address, token, &cancelled_path, &cancelled_request).await;
    tokio::time::timeout(Duration::from_secs(3), provider.search_entered())
        .await
        .expect("background admission must commit before its provider search is held");
    let writer = tokio::time::timeout(Duration::from_secs(3), pool.begin_with("BEGIN IMMEDIATE"))
        .await
        .expect("the owned writer must block only post-admission lease cleanup")
        .unwrap();
    drop(client);
    tokio::time::timeout(Duration::from_secs(3), provider.search_aborted())
        .await
        .expect("disconnect must abort the held provider read while lease cleanup is blocked");
    let foreground_request = json!({"request_id": "9a7789c6-c1f5-4d75-8fbb-5e67de9bfa4f"});
    *foreground = Some(start(address, token, &foreground_path, &foreground_request));
    entering.entered().await;
    assert!(
        !foreground.as_ref().unwrap().is_finished(),
        "the one permitted successor must remain pending behind owned cleanup"
    );
    assert_eq!(provider.counts(), ((0, 1, 0), 1, 0));

    let rejected: [Uuid; 4] = std::array::from_fn(|_| Uuid::new_v4());
    for request in [foreground_request.clone(), foreground_request.clone()]
        .into_iter()
        .chain(rejected.map(|request_id| json!({"request_id": request_id})))
    {
        surplus.spawn(send(
            address,
            token.to_owned(),
            foreground_path.clone(),
            request,
        ));
    }
    surplus.spawn(send(
        address,
        token.to_owned(),
        cancelled_path.clone(),
        cancelled_request.clone(),
    ));
    let returned = tokio::time::timeout(Duration::from_secs(1), async {
        let mut returned = 0;
        while let Some(joined) = surplus.join_next().await {
            let response = joined.expect("each surplus HTTP caller must join");
            assert_eq!(
                response.status,
                StatusCode::CONFLICT,
                "surplus cancellation handoff callers must receive controlled Busy"
            );
            assert!(
                response.json() == json!({"error": "Source resolution is busy. Try again."}),
                "Busy must return the controlled public response"
            );
            returned += 1;
        }
        returned
    })
    .await
    .expect(
        "surplus same-account retries must return Busy before blocked lease cleanup is released",
    );
    assert_eq!(returned, 7);
    assert!(
        !foreground.as_ref().unwrap().is_finished(),
        "rejecting surplus callers must preserve the permitted waiting successor"
    );
    assert_eq!(
        provider.counts(),
        ((0, 1, 0), 1, 0),
        "Busy must not dispatch provider work"
    );

    writer.rollback().await.unwrap();
    let accepted = tokio::time::timeout(Duration::from_secs(3), foreground.as_mut().unwrap())
        .await
        .expect("the permitted successor must finish promptly after cleanup can commit")
        .expect("the permitted HTTP successor must join");
    foreground.take();
    assert_eq!(accepted.status, StatusCode::OK);
    let accepted = accepted.json();
    assert_eq!(accepted["request_id"], foreground_request["request_id"]);
    assert_eq!(accepted["recording_id"], recordings[0]);
    assert_eq!(accepted["outcome"], "ready");
    assert_eq!(accepted["sources"].as_array().unwrap().len(), 1);
    assert_eq!(accepted["sources"][0]["ready"], true);
    assert_eq!(accepted["sources"][0]["recording_id"], recordings[0]);
    provider.release_search();
    tokio::time::timeout(Duration::from_secs(3), provider.search_released())
        .await
        .expect("the obsolete provider gate must release without reviving its caller");
    let cancelled = post(
        "cancelled_receipt",
        address,
        &cancelled_path,
        cancelled_request.clone(),
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert_eq!(cancelled["outcome"], "cancelled");
    assert_eq!(cancelled["recording_id"], recordings[1]);
    assert!(cancelled["sources"].as_array().unwrap().is_empty());
    let replay = post(
        "cancelled_receipt_replay",
        address,
        &cancelled_path,
        cancelled_request,
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert!(
        replay == cancelled,
        "cancelled receipt identities and outcome must remain immutable"
    );
    let replay = post(
        "foreground_receipt_replay",
        address,
        &foreground_path,
        foreground_request,
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert!(
        replay == accepted,
        "the waiting successor's exact request must retain its committed identities"
    );
    let mut identities = HashSet::from([accepted["resolution_id"].as_str().unwrap().to_owned()]);
    for request_id in rejected {
        let retry = post(
            "previously_busy_request",
            address,
            &foreground_path,
            json!({"request_id": request_id}),
            Some(token),
            StatusCode::OK,
        )
        .await;
        assert_eq!(retry["request_id"], request_id.to_string());
        assert_eq!(retry["recording_id"], recordings[0]);
        assert_eq!(retry["outcome"], "ready");
        assert!(
            identities.insert(retry["resolution_id"].as_str().unwrap().to_owned()),
            "Busy must not create aliases of the accepted receipt or other rejected requests"
        );
        assert!(
            retry["sources"] == accepted["sources"],
            "later explicit requests must preserve the existing ready source identity"
        );
    }
    assert_eq!(
        provider.counts(),
        ((1, 2, 0), 1, 0),
        "Busy, receipt replay and ready-source use must not repeat provider search, hydrate stale candidates or request audio"
    );
    let catalog = at_phase(
        "catalog_after",
        http::get(address, token, "/api/v1/recordings"),
    )
    .await;
    assert_eq!(catalog["items"].as_array().unwrap().len(), 2);
    for (index, recording) in recordings.iter().enumerate() {
        let item = catalog["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["id"].as_str() == Some(recording.as_str()))
            .unwrap();
        assert_eq!(item["available"], index == 0);
        let after = at_phase(
            "metadata_after",
            http::get(
                address,
                token,
                &format!("/api/v1/recordings/{recording}/metadata"),
            ),
        )
        .await;
        assert!(
            after["selected_observation"] == metadata[index]["selected_observation"]
                && after["title"] == metadata[index]["title"]
                && after["artist"] == metadata[index]["artist"],
            "cancellation handoff must preserve original metadata and observation identities"
        );
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
            "handoff and Busy must not create download intents"
        );
    }
    let encoded = json!([accepted, cancelled]).to_string();
    for private in [
        fixture::TARGET,
        fixture::CHOSEN,
        fixture::WAITING,
        fixture::WAITING_CANDIDATE,
        provider.url.as_str(),
    ] {
        assert!(
            !encoded.contains(private),
            "resolution responses must omit private provider identities and URLs"
        );
    }
}

fn start(
    address: SocketAddr,
    token: &str,
    path: &str,
    request: &Value,
) -> JoinHandle<http::Response> {
    tokio::spawn(send(
        address,
        token.to_owned(),
        path.to_owned(),
        request.clone(),
    ))
}

async fn send(address: SocketAddr, token: String, path: String, request: Value) -> http::Response {
    http::call(address, "POST", &path, Some(request), Some(&token)).await
}

async fn begin(address: SocketAddr, token: &str, path: &str, body: &Value) -> TcpStream {
    tokio::time::timeout(Duration::from_secs(3), async {
        let body = body.to_string();
        let mut client = TcpStream::connect(address).await.unwrap();
        client.write_all(format!("POST {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Type: application/json\r\nX-Hito-Request: 1\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        client
    }).await.expect("the owned background request must be sent")
}

async fn post(
    phase: &'static str,
    address: SocketAddr,
    path: &str,
    body: Value,
    token: Option<&str>,
    status: StatusCode,
) -> Value {
    let response = at_phase(phase, http::call(address, "POST", path, Some(body), token)).await;
    assert_eq!(response.status, status, "owned HTTP phase: {phase}");
    response.json()
}

async fn at_phase<T>(phase: &'static str, pending: impl Future<Output = T>) -> T {
    AssertUnwindSafe(pending)
        .catch_unwind()
        .await
        .unwrap_or_else(|_| panic!("owned HTTP phase failed: {phase}"))
}
