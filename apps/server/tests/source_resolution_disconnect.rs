#[path = "fixtures/source_resolution_disconnect.rs"]
mod fixture;
#[path = "fixtures/download_host.rs"]
mod host;
#[allow(dead_code)]
#[path = "fixtures/artwork_http.rs"]
mod http;

use axum::http::StatusCode;
use futures_util::FutureExt;
use serde_json::{Value, json};
use std::{future::Future, net::SocketAddr, panic::AssertUnwindSafe, time::Duration};
use tokio::{io::AsyncWriteExt, net::TcpStream};

#[tokio::test]
async fn disconnecting_background_resolution_releases_foreground_play_and_cannot_revive_its_receipt()
 {
    let directory = tempfile::tempdir().unwrap();
    let services = host::open(directory.path(), "resolution-disconnect-host").await;
    let mut server = host::Host::start(services).await;
    let mut provider = fixture::Provider::start().await;
    let outcome = tokio::time::timeout(
        Duration::from_secs(25),
        AssertUnwindSafe(exercise(server.address, &provider)).catch_unwind(),
    )
    .await;
    provider.release_search();
    server.request_shutdown();
    let joined = server.join().await;
    let provider_joined = provider.finish().await;
    match outcome {
        Ok(Ok(())) => {}
        Ok(Err(panic)) => std::panic::resume_unwind(panic),
        Err(_) => panic!("the owned disconnect workflow must finish within its deadline"),
    }
    assert!(joined && provider_joined, "owned servers must finish");
}

async fn exercise(address: SocketAddr, provider: &fixture::Provider) {
    let session = call(
        "setup",
        address,
        "/api/v1/setup",
        json!({"username": "owner", "password": "owned disconnect resolution password"}),
        None,
        StatusCode::CREATED,
    )
    .await;
    let token = session["token"].as_str().unwrap();
    let service = call(
        "offer_service",
        address,
        "/api/v1/plugins/services",
        json!({"name": "Owned disconnect service", "base_url": provider.url}),
        Some(token),
        StatusCode::CREATED,
    )
    .await;
    let installation = call("install", address, "/api/v1/plugins", json!({"service_id": service["id"], "service_revision": service["revision"], "bearer_token": null}), Some(token), StatusCode::CREATED).await;
    let adoption = format!(
        "/api/v1/plugins/{}/recordings",
        installation["id"].as_str().unwrap()
    );
    let mut originals = Vec::new();
    for (phase, provider_recording) in [
        ("adopt_foreground", fixture::TARGET),
        ("adopt_background", fixture::WAITING),
    ] {
        let original = call(phase, address, &adoption, json!({"installation_revision": installation["revision"], "provider_recording_id": provider_recording}), Some(token), StatusCode::OK).await;
        assert_eq!(original["available"], false);
        originals.push(original["id"].as_str().unwrap().to_owned());
    }
    assert_ne!(originals[0], originals[1]);
    let mut metadata = Vec::new();
    for (phase, recording) in ["metadata_before_foreground", "metadata_before_background"]
        .into_iter()
        .zip(&originals)
    {
        metadata.push(
            at_phase(
                phase,
                http::get(
                    address,
                    token,
                    &format!("/api/v1/recordings/{recording}/metadata"),
                ),
            )
            .await,
        );
    }
    let foreground_path = format!("/api/v1/recordings/{}/sources/resolve", originals[0]);
    let waiting_path = format!("/api/v1/recordings/{}/sources/resolve", originals[1]);
    let waiting_request = json!({"request_id": "5b96283b-f35a-43cc-8291-16dd6df50c69"});
    let socket = begin(address, &waiting_path, token, &waiting_request).await;
    tokio::time::timeout(Duration::from_secs(3), provider.search_entered())
        .await
        .expect("background resolution must enter the held provider search before disconnect");
    assert_eq!(provider.counts(), ((0, 1, 0), 1, 0));
    drop(socket);
    tokio::time::timeout(Duration::from_secs(3), provider.search_aborted())
        .await
        .expect(
            "closing the HTTP client must abort its held upstream read before the resolver timeout",
        );

    let foreground_request = json!({"request_id": "6de3652c-3882-4839-84a0-c194da90c9cf"});
    let foreground = call(
        "foreground_resolve_after_disconnect",
        address,
        &foreground_path,
        foreground_request.clone(),
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert_eq!(foreground["request_id"], foreground_request["request_id"]);
    assert_eq!(foreground["recording_id"], originals[0]);
    assert_eq!(foreground["outcome"], "ready");
    assert_eq!(foreground["sources"].as_array().unwrap().len(), 1);
    assert_eq!(foreground["sources"][0]["recording_id"], originals[0]);
    assert_eq!(foreground["sources"][0]["ready"], true);
    assert_eq!(provider.counts(), ((1, 2, 0), 1, 0));

    provider.release_search();
    tokio::time::timeout(Duration::from_secs(3), provider.search_released())
        .await
        .expect("the owned provider must settle its released search gate");
    let cancelled = call(
        "background_cancelled_receipt",
        address,
        &waiting_path,
        waiting_request.clone(),
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert_eq!(cancelled["request_id"], waiting_request["request_id"]);
    assert_eq!(cancelled["recording_id"], originals[1]);
    assert_eq!(cancelled["outcome"], "cancelled");
    assert!(cancelled["sources"].as_array().unwrap().is_empty());
    let replay = call(
        "background_cancelled_receipt_replay",
        address,
        &waiting_path,
        waiting_request,
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert!(
        replay == cancelled,
        "the exact cancelled receipt must remain immutable"
    );
    let replay = call(
        "foreground_committed_receipt_replay",
        address,
        &foreground_path,
        foreground_request,
        Some(token),
        StatusCode::OK,
    )
    .await;
    assert!(
        replay == foreground,
        "the committed foreground receipt must retain its identities"
    );
    assert_eq!(
        provider.counts(),
        ((1, 2, 0), 1, 0),
        "releasing the obsolete gate and replaying receipts must not hydrate a stale candidate or request audio"
    );

    let recordings = at_phase(
        "catalog_after_replay",
        http::get(address, token, "/api/v1/recordings"),
    )
    .await;
    let items = recordings["items"].as_array().unwrap();
    assert_eq!(
        items.len(),
        2,
        "resolution must retain exactly the adopted recordings"
    );
    for (index, recording) in originals.iter().enumerate() {
        let item = items
            .iter()
            .find(|item| item["id"].as_str() == Some(recording.as_str()))
            .unwrap();
        assert_eq!(item["available"], index == 0);
        let after = at_phase(
            "metadata_after_replay",
            http::get(
                address,
                token,
                &format!("/api/v1/recordings/{recording}/metadata"),
            ),
        )
        .await;
        assert!(
            after["selected_observation"] == metadata[index]["selected_observation"],
            "resolution must preserve selected metadata identity"
        );
        assert!(
            after["title"] == metadata[index]["title"]
                && after["artist"] == metadata[index]["artist"],
            "resolution must preserve the original display metadata"
        );
        let download = at_phase(
            "download_absence_after_replay",
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
            "resolution must not create a download intent"
        );
    }
    let waiting_sources = at_phase(
        "background_sources_after_replay",
        http::get(
            address,
            token,
            &format!("/api/v1/recordings/{}/sources", originals[1]),
        ),
    )
    .await;
    assert!(
        waiting_sources.as_array().unwrap().is_empty(),
        "a released obsolete search must not publish sources"
    );
    let encoded = json!([foreground, cancelled]).to_string();
    for private in [
        fixture::TARGET,
        fixture::CHOSEN,
        fixture::WAITING,
        fixture::WAITING_CANDIDATE,
        provider.url.as_str(),
    ] {
        assert!(
            !encoded.contains(private),
            "resolution responses must omit provider identities and URLs"
        );
    }
}

async fn begin(address: SocketAddr, path: &str, token: &str, body: &Value) -> TcpStream {
    tokio::time::timeout(Duration::from_secs(3), async {
        let body = body.to_string();
        let mut socket = TcpStream::connect(address).await.unwrap();
        socket.write_all(format!("POST {path} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Type: application/json\r\nX-Hito-Request: 1\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        socket
    })
    .await
    .expect("the owned background request must be sent")
}

async fn call(
    phase: &'static str,
    address: SocketAddr,
    path: &str,
    body: Value,
    token: Option<&str>,
    expected: StatusCode,
) -> Value {
    let response = at_phase(phase, http::call(address, "POST", path, Some(body), token)).await;
    assert_eq!(response.status, expected, "owned HTTP phase: {phase}");
    response.json()
}

async fn at_phase<T>(phase: &'static str, pending: impl Future<Output = T>) -> T {
    AssertUnwindSafe(pending)
        .catch_unwind()
        .await
        .unwrap_or_else(|_| panic!("owned HTTP phase did not finish: {phase}"))
}
