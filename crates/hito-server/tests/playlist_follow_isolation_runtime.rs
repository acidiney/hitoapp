#[path = "fixtures/artwork_http.rs"]
mod http;
use hito_core::test_support::content_service as provider;
#[path = "api/support.rs"]
#[allow(dead_code)]
mod support;

use axum::http::StatusCode;
use futures_util::FutureExt;
use http::{call, get};
use serde_json::{Value, json};
use std::{net::SocketAddr, panic::AssertUnwindSafe, sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::oneshot};
use uuid::Uuid;

#[tokio::test]
async fn a_slow_accounts_follow_does_not_block_another_accounts_publication() {
    let directory = tempfile::tempdir().unwrap();
    let services = support::services(directory.path(), &directory.path().join("music")).await;
    let providers = [
        provider::ProviderServer::start().await,
        provider::ProviderServer::start().await,
    ];
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let config = hito_server::Config {
        origin: format!("http://{address}"),
        ..Default::default()
    };
    let (stop, stopped) = oneshot::channel();
    let mut serving = tokio::spawn(hito_server::serve(
        listener,
        hito_server::router_with_config(services.clone(), config),
        services,
        async {
            let _ = stopped.await;
        },
    ));
    let outcome = AssertUnwindSafe(exercise(address, &providers))
        .catch_unwind()
        .await;
    let _ = stop.send(());
    let shutdown = tokio::time::timeout(Duration::from_secs(7), &mut serving).await;
    if shutdown.is_err() {
        serving.abort();
        let _ = serving.await;
        panic!("host shutdown must join HTTP and its background workers");
    }
    shutdown.unwrap().unwrap().unwrap();
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn exercise(address: SocketAddr, providers: &[provider::ProviderServer; 2]) {
    let owner = post(
        address,
        None,
        "/api/v1/setup",
        json!({"username": "owner", "password": "owned slow follow password"}),
        StatusCode::CREATED,
    )
    .await;
    let owner_token = owner["token"].as_str().unwrap();
    let credentials = json!({"username": "listener", "password": "owned ready follow password"});
    post(
        address,
        Some(owner_token),
        "/api/v1/users",
        credentials.clone(),
        StatusCode::CREATED,
    )
    .await;
    let listener = post(address, None, "/api/v1/login", credentials, StatusCode::OK).await;
    let listener_token = listener["token"].as_str().unwrap();
    assert_ne!(owner["user"]["id"], listener["user"]["id"]);
    let tokens = [owner_token, listener_token];
    let mut playlists = Vec::new();
    let mut originals = Vec::new();
    let mut recordings = Vec::new();
    let mut private = Vec::new();
    let mut detail_requests = Vec::new();
    for (index, provider) in providers.iter().enumerate() {
        provider.reply("v1/manifest", json!({
            "id": format!("org.hito.follow-isolation-{index}"),
            "name": "Owned isolated follow", "protocol_version": 1, "capabilities": ["playlists"]
        })).await;
        snapshot(provider, index, "initial", false).await;
        let service = post(
            address,
            Some(owner_token),
            "/api/v1/plugins/services",
            json!({"name": "Owned isolated follow", "base_url": provider.url}),
            StatusCode::CREATED,
        )
        .await;
        let installation = post(
            address,
            Some(tokens[index]),
            "/api/v1/plugins",
            json!({"service_id": service["id"], "service_revision": service["revision"]}),
            StatusCode::CREATED,
        )
        .await;
        let followed = post(
            address,
            Some(tokens[index]),
            &format!(
                "/api/v1/plugins/{}/playlists/follow",
                installation["id"].as_str().unwrap()
            ),
            json!({
                "request_id": Uuid::new_v4(), "installation_revision": installation["revision"],
                "playlist_reference": format!("https://provider.invalid/owned-selection-{index}"),
                "interval": "hourly"
            }),
            StatusCode::OK,
        )
        .await;
        let playlist = followed["playlist_id"].as_str().unwrap().to_owned();
        let original = get(
            address,
            tokens[index],
            &format!("/api/v1/playlists/{playlist}"),
        )
        .await;
        let recording = original["entries"][0]["recording_id"]
            .as_str()
            .unwrap()
            .to_owned();
        post(
            address,
            Some(tokens[index]),
            "/api/v1/sync",
            json!({"device_id": format!("follow-isolation-{index}"), "operations": [
                {"id": Uuid::new_v4(), "action": {
                    "kind": "recording_title_set", "recording_id": recording,
                    "title": format!("My private title {index}")
                }},
                {"id": Uuid::new_v4(), "action": {
                    "kind": "like_set", "recording_id": recording, "liked": true
                }},
                {"id": Uuid::new_v4(), "action": {
                    "kind": "progress_set", "recording_id": recording,
                    "position_ms": 1200 + index * 1000, "speed": 1.25
                }}
            ]}),
            StatusCode::OK,
        )
        .await;
        let original = get(
            address,
            tokens[index],
            &format!("/api/v1/playlists/{playlist}"),
        )
        .await;
        private.push(private_state(address, tokens[index], &recording).await);
        recordings.push(recording);
        playlists.push(playlist);
        originals.push(original);
        detail_requests.push(provider.requests("v1/playlists/detail").await.len());
        snapshot(provider, index, "updated", true).await;
    }
    assert_eq!(private[0]["identity"], owner["user"]);
    assert_eq!(private[1]["identity"], listener["user"]);
    for (index, state) in private.iter().enumerate() {
        assert_eq!(
            state["metadata"]["title"]["personal"],
            format!("My private title {index}")
        );
        assert_eq!(state["recording"]["liked"], true);
        assert_eq!(
            state["activity"]["history"][0]["position_ms"],
            1200 + index * 1000
        );
    }
    let paths = playlists
        .iter()
        .map(|playlist| format!("/api/v1/playlists/{playlist}/follow"))
        .collect::<Vec<_>>();
    let gate = providers[0].hold("v1/playlists/detail").await;
    let _release = ReleaseGate(gate.clone());
    refresh(address, tokens[0], &paths[0]).await;
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .expect("the slow account must reach its held provider detail");
    let slow = get(address, tokens[0], &paths[0]).await;
    assert_eq!(slow["status"], "running");
    assert_eq!(slow["snapshot"], "initial");

    refresh(address, tokens[1], &paths[1]).await;
    let published =
        tokio::time::timeout(Duration::from_secs(4), ready(address, tokens[1], &paths[1]))
            .await
            .expect("the second account must publish while the first provider remains held");
    assert_eq!(published["snapshot"], "updated");
    assert_eq!(get(address, tokens[0], &paths[0]).await, slow);
    assert_eq!(
        get(
            address,
            tokens[0],
            &format!("/api/v1/playlists/{}", playlists[0])
        )
        .await,
        originals[0]
    );
    let updated = get(
        address,
        tokens[1],
        &format!("/api/v1/playlists/{}", playlists[1]),
    )
    .await;
    assert_eq!(updated["origin"]["snapshot"], "updated");
    assert_eq!(updated["entries"].as_array().unwrap().len(), 2);
    assert_eq!(
        updated["entries"][0]["entry_id"],
        originals[1]["entries"][0]["entry_id"]
    );
    assert_eq!(updated["entries"][0]["recording_id"], recordings[1]);
    assert_ne!(updated["entries"][1]["recording_id"], recordings[1]);
    for (index, &token) in tokens.iter().enumerate() {
        assert_eq!(
            private_state(address, token, &recordings[index]).await,
            private[index]
        );
        let denied = call(address, "GET", &paths[1 - index], None, Some(token)).await;
        assert_eq!(denied.status, StatusCode::NOT_FOUND);
        assert!(!String::from_utf8_lossy(&denied.body).contains(&playlists[1 - index]));
    }
    gate.open();
    let completed =
        tokio::time::timeout(Duration::from_secs(4), ready(address, tokens[0], &paths[0]))
            .await
            .expect("the slow account must finish after its provider is released");
    assert_eq!(completed["snapshot"], "updated");
    for (index, provider) in providers.iter().enumerate() {
        assert_eq!(
            private_state(address, tokens[index], &recordings[index]).await,
            private[index]
        );
        let requests = provider.requests("v1/playlists/detail").await;
        assert_eq!(requests.len(), detail_requests[index] + 2);
        assert!(
            requests[detail_requests[index]..]
                .iter()
                .all(|(_, body)| body
                    == &json!({
                        "kind": "canonical", "playlist_id": format!("playlist/owned-{index}")
                    }))
        );
        assert!(provider.requests("v1/sources").await.is_empty());
        assert!(provider.requests("v1/audio").await.is_empty());
    }
}

async fn snapshot(provider: &provider::ProviderServer, index: usize, version: &str, changed: bool) {
    let mut items = vec![recording(index, "retained")];
    if changed {
        items.push(recording(index, "added"));
    }
    provider
        .reply(
            "v1/playlists/detail",
            json!({
                "id": format!("playlist/owned-{index}"), "name": "Owned isolated selection",
                "snapshot": version, "total_count": items.len(),
                "attribution": {"label": "Owned isolated provider"}
            }),
        )
        .await;
    provider
        .reply(
            "v1/playlists/items",
            json!({
                "playlist_id": format!("playlist/owned-{index}"), "snapshot": version,
                "items": items, "next_cursor": null
            }),
        )
        .await;
}

fn recording(index: usize, occurrence: &str) -> Value {
    json!({"occurrence_id": occurrence, "item": {"kind": "recording", "recording": {
        "id": format!("recording/{index}/{occurrence}"),
        "title": format!("Owned item {index} {occurrence}"), "artist": "Owned isolated artist"
    }}})
}

async fn refresh(address: SocketAddr, token: &str, path: &str) {
    post(
        address,
        Some(token),
        &format!("{path}/refresh"),
        json!({"request_id": Uuid::new_v4()}),
        StatusCode::ACCEPTED,
    )
    .await;
}

async fn ready(address: SocketAddr, token: &str, path: &str) -> Value {
    loop {
        let status = get(address, token, path).await;
        if status["status"] == "ready" {
            assert!(status["error"].is_null());
            return status;
        }
        assert!(matches!(
            status["status"].as_str(),
            Some("queued" | "running")
        ));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn private_state(address: SocketAddr, token: &str, recording: &str) -> Value {
    let catalog = get(address, token, "/api/v1/recordings").await;
    let saved = catalog["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == recording)
        .unwrap();
    json!({
        "identity": get(address, token, "/api/v1/me").await,
        "metadata": get(address, token, &format!("/api/v1/recordings/{recording}/metadata")).await,
        "activity": get(address, token, "/api/v1/activity").await,
        "recording": saved,
        "sources": get(address, token, &format!("/api/v1/recordings/{recording}/sources")).await
    })
}

async fn post(
    address: SocketAddr,
    token: Option<&str>,
    path: &str,
    body: Value,
    expected: StatusCode,
) -> Value {
    let response = call(address, "POST", path, Some(body), token).await;
    assert_eq!(response.status, expected);
    assert_eq!(response.headers["content-type"], "application/json");
    response.json()
}

struct ReleaseGate(Arc<provider::Gate>);

impl Drop for ReleaseGate {
    fn drop(&mut self) {
        self.0.open();
    }
}
