#[path = "../fixtures/artwork_http.rs"]
mod http;
#[path = "../fixtures/artwork_provider.rs"]
mod provider;
#[path = "../api/support.rs"]
#[allow(dead_code)]
mod support;

use axum::http::StatusCode;
use futures_util::FutureExt;
use http::{call, get};
use serde_json::{Value, json};
use std::{net::SocketAddr, panic::AssertUnwindSafe, time::Duration};
use tokio::{net::TcpListener, sync::oneshot};
use uuid::Uuid;

#[tokio::test]
async fn the_host_fetches_private_recording_artwork_without_changing_saved_collection_state() {
    let directory = tempfile::tempdir().unwrap();
    let services = support::services(directory.path(), &directory.path().join("music")).await;
    let provider = provider::ArtworkServer::start().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let config = hito_server::Config {
        origin: format!("http://{address}"),
        ..Default::default()
    };
    let (stop, stopped) = oneshot::channel();
    let serving = tokio::spawn(hito_server::serve(
        listener,
        hito_server::router_with_config(services.clone(), config),
        services,
        async {
            let _ = stopped.await;
        },
    ));
    let outcome = AssertUnwindSafe(exercise(address, &provider))
        .catch_unwind()
        .await;
    let _ = stop.send(());
    tokio::time::timeout(Duration::from_secs(7), serving)
        .await
        .expect("host shutdown must join HTTP and both background workers")
        .unwrap()
        .unwrap();
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn exercise(address: SocketAddr, provider: &provider::ArtworkServer) {
    let response = call(
        address,
        "POST",
        "/api/v1/setup",
        Some(json!({"username": "owner", "password": "owned artwork runtime password"})),
        None,
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let session = response.json();
    let token = session["token"].as_str().unwrap();
    let response = call(
        address,
        "POST",
        "/api/v1/plugins/services",
        Some(json!({"name": "Owned artwork runtime", "base_url": provider.url})),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let service = response.json();
    let response = call(
        address,
        "POST",
        "/api/v1/plugins",
        Some(json!({"service_id": service["id"], "service_revision": service["revision"]})),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let installation = response.json();
    let response = call(
        address,
        "POST",
        &format!(
            "/api/v1/plugins/{}/playlists/import",
            installation["id"].as_str().unwrap()
        ),
        Some(json!({
            "request_id": Uuid::new_v4(),
            "installation_revision": installation["revision"],
            "playlist_reference": "https://provider.invalid/owned-selection"
        })),
        Some(token),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK);
    let imported = response.json();
    let playlist = imported["playlist_id"].as_str().unwrap();
    let detail = get(address, token, &format!("/api/v1/playlists/{playlist}")).await;
    let entries = detail["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 2);
    assert_ne!(entries[0]["entry_id"], entries[1]["entry_id"]);
    assert_eq!(entries[0]["recording_id"], entries[1]["recording_id"]);
    let recording = entries[0]["recording_id"].as_str().unwrap();
    let image_path = format!("/api/v1/recordings/{recording}/artwork");
    let fetch_path = format!("{image_path}/fetch");
    assert_eq!(
        call(address, "GET", &image_path, None, Some(token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    let operations = [
        json!({"kind": "recording_title_set", "recording_id": recording, "title": "My retained title"}),
        json!({"kind": "recording_artist_set", "recording_id": recording, "artist": "My retained artist"}),
        json!({"kind": "like_set", "recording_id": recording, "liked": true}),
        json!({"kind": "progress_set", "recording_id": recording, "position_ms": 4200, "speed": 1.25}),
    ].into_iter().map(|action| json!({"id": Uuid::new_v4(), "action": action})).collect::<Vec<_>>();
    assert_eq!(
        call(
            address,
            "POST",
            "/api/v1/sync",
            Some(json!({
                "device_id": "artwork-runtime", "operations": operations
            })),
            Some(token)
        )
        .await
        .status,
        StatusCode::OK
    );
    let other_credentials =
        json!({"username": "other", "password": "other artwork runtime password"});
    assert_eq!(
        call(
            address,
            "POST",
            "/api/v1/users",
            Some(other_credentials.clone()),
            Some(token)
        )
        .await
        .status,
        StatusCode::CREATED
    );
    let other = call(
        address,
        "POST",
        "/api/v1/login",
        Some(other_credentials),
        None,
    )
    .await;
    assert_eq!(other.status, StatusCode::OK);
    let other_session = other.json();
    let other_token = other_session["token"].as_str().unwrap();
    let before = saved_state(address, token, playlist, recording).await;
    assert_eq!(before["catalog"]["total"], 1);
    assert_eq!(before["catalog"]["items"][0]["id"], recording);
    assert_eq!(before["catalog"]["items"][0]["liked"], true);
    assert_eq!(before["sources"], json!([]));
    assert_eq!(before["metadata"]["title"]["personal"], "My retained title");
    assert_eq!(
        before["metadata"]["artist"]["personal"],
        "My retained artist"
    );
    assert_eq!(before["activity"]["history"][0]["position_ms"], 4200);
    assert_eq!(before["activity"]["history"][0]["speed"], 1.25);
    let requests_before = provider.requests().await;

    let response = call(address, "POST", &fetch_path, None, Some(token)).await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    let queued = response.json();
    assert_eq!(queued["recording_id"], recording);
    assert_eq!(queued["status"], "queued");
    assert_eq!(queued["attempts"], 0);
    assert!(queued["error"].is_null());
    assert!(queued["created_at"].is_i64());
    assert!(queued["updated_at"].is_i64());
    assert_eq!(
        queued
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "attempts",
            "created_at",
            "error",
            "id",
            "recording_id",
            "status",
            "updated_at"
        ]
    );
    let job = queued["id"].as_str().unwrap();
    Uuid::parse_str(job).unwrap();
    let job_path = format!("/api/v1/artwork-fetches/{job}");
    tokio::time::timeout(Duration::from_secs(3), provider.image_entered())
        .await
        .expect("the real host worker must request the held image");
    let running = get(address, token, &job_path).await;
    assert_eq!(running["id"], job);
    assert_eq!(running["status"], "running");
    let retry = call(address, "POST", &fetch_path, None, Some(token)).await;
    assert_eq!(retry.status, StatusCode::ACCEPTED);
    assert_eq!(retry.json()["id"], job);
    assert_eq!(
        call(address, "GET", &image_path, None, Some(token))
            .await
            .status,
        StatusCode::NOT_FOUND
    );
    provider.release_image();
    let ready = tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let receipt = get(address, token, &job_path).await;
            if receipt["status"] == "ready" {
                break receipt;
            }
            assert!(matches!(
                receipt["status"].as_str(),
                Some("queued" | "running")
            ));
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the real host worker must retain the validated cover");
    assert_eq!(ready["attempts"], 1);
    let response = call(address, "GET", &image_path, None, Some(token)).await;
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.headers["content-type"], "image/png");
    assert_eq!(response.headers["cache-control"], "private, no-store");
    assert_eq!(response.body, provider::COVER);
    let retry = call(address, "POST", &fetch_path, None, Some(token)).await;
    assert_eq!(retry.status, StatusCode::ACCEPTED);
    assert_eq!(retry.json(), ready);
    for (token, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(other_token), StatusCode::NOT_FOUND),
    ] {
        for path in [&job_path, &image_path] {
            let response = call(address, "GET", path, None, token).await;
            assert_eq!(response.status, expected);
            assert!(!String::from_utf8_lossy(&response.body).contains(recording));
            assert!(!String::from_utf8_lossy(&response.body).contains(job));
        }
    }
    assert_eq!(
        saved_state(address, token, playlist, recording).await,
        before
    );
    let requests = provider.requests().await;
    assert_eq!(
        &requests[..requests_before.len()],
        requests_before.as_slice()
    );
    assert_eq!(
        &requests[requests_before.len()..],
        &[json!({
            "method": "POST", "path": "v1/artwork",
            "body": {"entity": "recording", "provider_id": provider::PROVIDER_RECORDING_ID}
        })]
    );
}

async fn saved_state(address: SocketAddr, token: &str, playlist: &str, recording: &str) -> Value {
    json!({
        "catalog": get(address, token, "/api/v1/recordings").await,
        "playlists": get(address, token, "/api/v1/playlists").await,
        "playlist": get(address, token, &format!("/api/v1/playlists/{playlist}")).await,
        "metadata": get(address, token, &format!("/api/v1/recordings/{recording}/metadata")).await,
        "sources": get(address, token, &format!("/api/v1/recordings/{recording}/sources")).await,
        "activity": get(address, token, "/api/v1/activity").await,
        "events": get(address, token, "/api/v1/sync?cursor=0").await,
    })
}
