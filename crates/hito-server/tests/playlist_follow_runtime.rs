#[path = "fixtures/artwork_http.rs"]
mod http;
#[path = "../../hito-core/tests/fixtures/content_service.rs"]
mod provider;
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

const PLAYLIST: &str = "playlist/runtime-follow";
const REFERENCE: &str = "https://provider.invalid/runtime-selection";

#[tokio::test]
async fn the_host_refreshes_a_follow_without_changing_an_independent_editable_copy() {
    let directory = tempfile::tempdir().unwrap();
    let services = support::services(directory.path(), &directory.path().join("music")).await;
    let provider = provider::ProviderServer::start().await;
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
    let outcome = AssertUnwindSafe(exercise(address, &provider))
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

async fn exercise(address: SocketAddr, provider: &provider::ProviderServer) {
    provider
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.follow-runtime", "name": "Owned follow runtime",
                "protocol_version": 1, "capabilities": ["playlists"]
            }),
        )
        .await;
    snapshot(
        provider,
        "snapshot-one",
        vec![
            recording("first", "A"),
            recording("duplicate", "A"),
            recording("retained", "B"),
            recording("removed", "D"),
            unknown(),
        ],
    )
    .await;
    let response = call(
        address,
        "POST",
        "/api/v1/setup",
        Some(json!({"username": "owner", "password": "owned follow runtime password"})),
        None,
    )
    .await;
    assert_eq!(response.status, StatusCode::CREATED);
    let session = response.json();
    let token = session["token"].as_str().unwrap();
    let service = post(
        address,
        token,
        "/api/v1/plugins/services",
        json!({"name": "Owned follow runtime", "base_url": provider.url}),
        StatusCode::CREATED,
    )
    .await;
    let installation = post(
        address,
        token,
        "/api/v1/plugins",
        json!({"service_id": service["id"], "service_revision": service["revision"]}),
        StatusCode::CREATED,
    )
    .await;
    let provider_path = format!(
        "/api/v1/plugins/{}/playlists",
        installation["id"].as_str().unwrap()
    );
    let copied = post(
        address,
        token,
        &format!("{provider_path}/import"),
        json!({
            "request_id": Uuid::new_v4(), "installation_revision": installation["revision"],
            "playlist_reference": REFERENCE
        }),
        StatusCode::OK,
    )
    .await;
    let copy_path = format!(
        "/api/v1/playlists/{}",
        copied["playlist_id"].as_str().unwrap()
    );
    post(
        address,
        token,
        "/api/v1/sync",
        json!({"device_id": "follow-runtime", "operations": [{
            "id": Uuid::new_v4(), "action": {
                "kind": "playlist_rename", "playlist_id": copied["playlist_id"],
                "name": "My independent selection"
            }
        }]}),
        StatusCode::OK,
    )
    .await;
    let copy_before = get(address, token, &copy_path).await;
    assert_eq!(copy_before["playlist"]["kind"], "local");
    assert_eq!(copy_before["playlist"]["name"], "My independent selection");

    let followed = post(
        address,
        token,
        &format!("{provider_path}/follow"),
        json!({
            "request_id": Uuid::new_v4(), "installation_revision": installation["revision"],
            "playlist_reference": REFERENCE, "interval": "hourly"
        }),
        StatusCode::OK,
    )
    .await;
    assert_ne!(followed["playlist_id"], copied["playlist_id"]);
    let playlist = followed["playlist_id"].as_str().unwrap();
    let detail_path = format!("/api/v1/playlists/{playlist}");
    let status_path = format!("{detail_path}/follow");
    let before = get(address, token, &detail_path).await;
    assert_eq!(before["playlist"]["kind"], "followed");
    let original = before["entries"].as_array().unwrap();
    assert_eq!(original.len(), 5);
    assert_ne!(original[0]["entry_id"], original[1]["entry_id"]);
    assert_eq!(original[0]["recording_id"], original[1]["recording_id"]);
    let ready = get(address, token, &status_path).await;
    assert_eq!(ready["status"], "ready");
    assert_eq!(ready["snapshot"], "snapshot-one");
    let detail_requests = provider.requests("v1/playlists/detail").await.len();
    let item_requests = provider.requests("v1/playlists/items").await.len();

    snapshot(
        provider,
        "snapshot-two",
        vec![
            recording("retained", "B"),
            recording("duplicate", "A"),
            recording("new", "C"),
            recording("first", "A"),
            unknown(),
        ],
    )
    .await;
    let gate = provider.hold("v1/playlists/detail").await;
    let _release = ReleaseGate(gate.clone());
    let refresh_body = json!({"request_id": Uuid::new_v4()});
    let refresh_path = format!("{status_path}/refresh");
    let refresh = post(
        address,
        token,
        &refresh_path,
        refresh_body.clone(),
        StatusCode::ACCEPTED,
    )
    .await;
    assert_eq!(refresh["playlist_id"], playlist);
    Uuid::parse_str(refresh["job_id"].as_str().unwrap()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .expect("the real host worker must request the held follow snapshot");
    let running = get(address, token, &status_path).await;
    assert_eq!(running["status"], "running");
    assert_eq!(running["snapshot"], "snapshot-one");
    assert_eq!(get(address, token, &detail_path).await, before);
    assert_eq!(
        post(
            address,
            token,
            &refresh_path,
            refresh_body,
            StatusCode::ACCEPTED
        )
        .await,
        refresh
    );
    gate.open();
    let updated = tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let status = get(address, token, &status_path).await;
            if status["status"] == "ready" {
                break status;
            }
            assert!(matches!(
                status["status"].as_str(),
                Some("queued" | "running")
            ));
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the real host worker must publish the complete follow snapshot");
    assert_eq!(updated["snapshot"], "snapshot-two");
    assert_eq!(updated["interval"], "hourly");
    assert!(updated["error"].is_null());
    assert_eq!(
        updated["next_refresh_at"].as_i64().unwrap()
            - updated["last_refreshed_at"].as_i64().unwrap(),
        3600
    );
    let after = get(address, token, &detail_path).await;
    assert_eq!(after["origin"]["snapshot"], "snapshot-two");
    let entries = after["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 5);
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry["position"].clone())
            .collect::<Vec<_>>(),
        [json!(0), json!(1), json!(2), json!(3), json!(4)]
    );
    assert_eq!(
        entries
            .iter()
            .map(|entry| entry["title"].clone())
            .collect::<Vec<_>>(),
        [
            json!("B"),
            json!("A"),
            json!("C"),
            json!("A"),
            json!("Unknown provider item")
        ]
    );
    for (current, previous) in [(0, 2), (1, 1), (3, 0)] {
        assert_eq!(entries[current]["entry_id"], original[previous]["entry_id"]);
        assert_eq!(
            entries[current]["recording_id"],
            original[previous]["recording_id"]
        );
    }
    for entry in [&entries[2], &entries[4]] {
        assert!(
            original
                .iter()
                .all(|old| old["entry_id"] != entry["entry_id"])
        );
    }
    assert!(
        entries
            .iter()
            .all(|entry| entry["entry_id"] != original[3]["entry_id"])
    );
    assert!(entries.iter().all(|entry| entry["available"] == false));
    assert_eq!(get(address, token, &copy_path).await, copy_before);
    let catalog = get(address, token, "/api/v1/recordings").await;
    assert_eq!(catalog["total"], 4);
    for recording in catalog["items"].as_array().unwrap() {
        let path = format!(
            "/api/v1/recordings/{}/sources",
            recording["id"].as_str().unwrap()
        );
        assert_eq!(get(address, token, &path).await, json!([]));
    }
    let requests = provider.requests("v1/playlists/detail").await;
    assert_eq!(requests.len(), detail_requests + 2);
    assert!(
        requests[detail_requests..]
            .iter()
            .all(|(_, body)| { body == &json!({"kind": "canonical", "playlist_id": PLAYLIST}) })
    );
    assert_eq!(
        provider.requests("v1/playlists/items").await.len(),
        item_requests + 1
    );
    for path in [
        "v1/search",
        "v1/recordings/detail",
        "v1/sources",
        "v1/audio",
    ] {
        assert!(provider.requests(path).await.is_empty());
    }
}

fn recording(occurrence: &str, id: &str) -> Value {
    json!({"occurrence_id": occurrence, "item": {"kind": "recording", "recording": {
        "id": id, "title": id, "artist": "Owned runtime artist"
    }}})
}

fn unknown() -> Value {
    json!({"occurrence_id": "unknown", "item": {"kind": "unknown"}})
}

async fn snapshot(provider: &provider::ProviderServer, snapshot: &str, items: Vec<Value>) {
    provider
        .reply(
            "v1/playlists/detail",
            json!({
                "id": PLAYLIST, "name": "Owned runtime selection", "snapshot": snapshot,
                "total_count": items.len(), "attribution": {"label": "Owned runtime provider"}
            }),
        )
        .await;
    provider
        .reply(
            "v1/playlists/items",
            json!({
                "playlist_id": PLAYLIST, "snapshot": snapshot, "items": items, "next_cursor": null
            }),
        )
        .await;
}

async fn post(
    address: SocketAddr,
    token: &str,
    path: &str,
    body: Value,
    expected: StatusCode,
) -> Value {
    let response = call(address, "POST", path, Some(body), Some(token)).await;
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
