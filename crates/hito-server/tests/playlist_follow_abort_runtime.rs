#[path = "fixtures/artwork_http.rs"]
mod http;
#[path = "../../hito-core/tests/fixtures/content_service.rs"]
mod provider;

use axum::http::StatusCode;
use futures_util::FutureExt;
use hito_core::{
    CoreConfig, Credentials, FollowProviderPlaylist, HostDeviceId, InstallPlugin,
    OfferPluginService, PlaylistFollowInterval, PlaylistFollowStatus, Services,
};
use http::{call, get};
use serde_json::{Value, json};
use std::{net::SocketAddr, panic::AssertUnwindSafe, path::Path, sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
use uuid::Uuid;

const PLAYLIST: &str = "playlist/aborted-host";

#[tokio::test]
async fn aborting_the_public_host_future_cancels_its_held_follow_and_preserves_restart_work() {
    let directory = tempfile::tempdir().unwrap();
    let provider = provider::ProviderServer::start().await;
    let mut hosts = Vec::new();
    let outcome = AssertUnwindSafe(exercise(directory.path(), &provider, &mut hosts))
        .catch_unwind()
        .await;
    for host in &mut hosts {
        host.request_shutdown();
        if !host.finish().await {
            if let Some(task) = host.task.take() {
                task.abort();
                let _ = task.await;
            }
            panic!("every owned host must finish cleanup and close its listener");
        }
    }
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

async fn exercise(directory: &Path, provider: &provider::ProviderServer, hosts: &mut Vec<Host>) {
    let services = open(directory).await;
    let session = services
        .identity
        .setup(Credentials {
            username: "owner".into(),
            password: "owned aborted host password".into(),
        })
        .await
        .unwrap();
    let token = session.token;
    let actor = services.identity.authenticate(&token).await.unwrap();
    provider
        .reply(
            "v1/manifest",
            json!({
                "id": "org.hito.follow-abort", "name": "Owned aborted host provider",
                "protocol_version": 1, "capabilities": ["playlists"]
            }),
        )
        .await;
    snapshot(
        provider,
        "initial",
        vec![
            recording("first", "A"),
            recording("duplicate", "A"),
            recording("removed", "B"),
        ],
    )
    .await;
    let service = services
        .plugins
        .offer_service(
            &actor,
            OfferPluginService {
                name: "Owned aborted host provider".into(),
                base_url: provider.url.clone(),
            },
        )
        .await
        .unwrap();
    let installation = services
        .plugins
        .install(
            &actor,
            InstallPlugin {
                service_id: service.id,
                service_revision: service.revision,
                bearer_token: None,
            },
        )
        .await
        .unwrap();
    let followed = services
        .collection
        .follow_provider_playlist(
            &actor,
            &installation.id,
            FollowProviderPlaylist {
                request_id: Uuid::new_v4(),
                installation_revision: installation.revision,
                playlist_reference: "https://provider.invalid/aborted-host-selection".into(),
                interval: PlaylistFollowInterval::Hourly,
            },
        )
        .await
        .unwrap();
    let playlist = followed.playlist_id.to_string();
    let before = serde_json::to_value(
        services
            .collection
            .playlist(&actor, &playlist)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_ne!(
        before["entries"][0]["entry_id"],
        before["entries"][1]["entry_id"]
    );
    assert_eq!(
        before["entries"][0]["recording_id"],
        before["entries"][1]["recording_id"]
    );
    let ready = services
        .collection
        .playlist_follow(&actor, &playlist)
        .await
        .unwrap();
    assert_eq!(ready.status, PlaylistFollowStatus::Ready);
    let requests_before = provider.requests("v1/playlists/detail").await.len();
    hosts.push(Host::start(services.clone()).await);
    let address = hosts[0].address;
    let detail_path = format!("/api/v1/playlists/{playlist}");
    let status_path = format!("{detail_path}/follow");
    snapshot(
        provider,
        "updated",
        vec![
            recording("duplicate", "A"),
            recording("added", "C"),
            recording("first", "A"),
        ],
    )
    .await;
    let gate = provider.hold("v1/playlists/detail").await;
    let _release = ReleaseGate(gate.clone());
    let response = call(
        address,
        "POST",
        &format!("{status_path}/refresh"),
        Some(json!({"request_id": Uuid::new_v4()})),
        Some(&token),
    )
    .await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    assert_eq!(response.headers["content-type"], "application/json");
    assert_eq!(response.json()["playlist_id"], playlist);
    tokio::time::timeout(Duration::from_secs(3), gate.entered())
        .await
        .expect("the public host must reach the held provider response");
    let running = get(address, &token, &status_path).await;
    assert_eq!(running["status"], "running");
    assert_eq!(running["snapshot"], "initial");
    assert_eq!(get(address, &token, &detail_path).await, before);

    hosts[0].abort_public_future().await;
    assert!(
        hosts[0].stop.is_some(),
        "the explicit shutdown sender must remain retained"
    );
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let state = services
                .collection
                .playlist_follow(&actor, &playlist)
                .await
                .unwrap();
            if state.status == PlaylistFollowStatus::Queued
                && tokio::net::TcpStream::connect(address).await.is_err()
            {
                assert_eq!(state.snapshot, "initial");
                assert!(state.error.is_none());
                assert_eq!(state.last_refreshed_at, ready.last_refreshed_at);
                assert_eq!(state.next_refresh_at, ready.next_refresh_at);
                break;
            }
            assert!(matches!(
                state.status,
                PlaylistFollowStatus::Running | PlaylistFollowStatus::Queued
            ));
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("aborting the public future must close the listener and requeue its held work");
    assert!(
        tokio::time::timeout(Duration::from_millis(1250), gate.entered())
            .await
            .is_err(),
        "the cancelled host must not start another provider request on its next polling cycle"
    );
    assert_eq!(
        provider.requests("v1/playlists/detail").await.len(),
        requests_before + 1
    );
    assert_eq!(
        serde_json::to_value(
            services
                .collection
                .playlist(&actor, &playlist)
                .await
                .unwrap()
        )
        .unwrap(),
        before
    );
    drop(services);

    let reopened = open(directory).await;
    let retained_actor = reopened
        .identity
        .authenticate(&token)
        .await
        .expect("the original session must authenticate after reopening the backend");
    assert_eq!(retained_actor.user().id, session.user.id);
    let retained = reopened
        .collection
        .playlist_follow(&retained_actor, &playlist)
        .await
        .unwrap();
    assert_eq!(retained.status, PlaylistFollowStatus::Queued);
    assert_eq!(retained.snapshot, "initial");
    assert!(retained.error.is_none());
    assert_eq!(
        serde_json::to_value(
            reopened
                .collection
                .playlist(&retained_actor, &playlist)
                .await
                .unwrap()
        )
        .unwrap(),
        before
    );
    gate.open();
    hosts.push(Host::start(reopened).await);
    let restarted = hosts[1].address;
    let completed = tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let state = get(restarted, &token, &status_path).await;
            if state["status"] == "ready" {
                break state;
            }
            assert!(matches!(
                state["status"].as_str(),
                Some("queued" | "running")
            ));
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the next host must publish retained work without a new enqueue");
    assert_eq!(completed["snapshot"], "updated");
    assert!(completed["error"].is_null());
    let after = get(restarted, &token, &detail_path).await;
    assert_eq!(after["origin"]["snapshot"], "updated");
    let entries = after["entries"].as_array().unwrap();
    let previous = before["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0]["entry_id"], previous[1]["entry_id"]);
    assert_eq!(entries[2]["entry_id"], previous[0]["entry_id"]);
    assert_eq!(entries[0]["recording_id"], previous[1]["recording_id"]);
    assert_eq!(entries[2]["recording_id"], previous[0]["recording_id"]);
    assert!(
        previous
            .iter()
            .all(|entry| entry["entry_id"] != entries[1]["entry_id"])
    );
    assert!(
        entries
            .iter()
            .all(|entry| entry["entry_id"] != previous[2]["entry_id"])
    );
    assert_eq!(entries[1]["title"], "C");
    assert!(entries.iter().all(|entry| entry["available"] == false));
    let requests = provider.requests("v1/playlists/detail").await;
    assert_eq!(requests.len(), requests_before + 3);
    assert!(requests[requests_before..].iter().all(|(_, body)| body
        == &json!({
            "kind": "canonical", "playlist_id": PLAYLIST
        })));
    assert!(provider.requests("v1/sources").await.is_empty());
    assert!(provider.requests("v1/audio").await.is_empty());
}

async fn open(directory: &Path) -> Services {
    hito_core::open(CoreConfig::new(
        directory.join("hito.db"),
        directory.join("music"),
        HostDeviceId::new("follow-aborted-host").unwrap(),
    ))
    .await
    .unwrap()
}

fn recording(occurrence: &str, id: &str) -> Value {
    json!({"occurrence_id": occurrence, "item": {"kind": "recording", "recording": {
        "id": id, "title": id, "artist": "Owned aborted host artist"
    }}})
}

async fn snapshot(provider: &provider::ProviderServer, version: &str, items: Vec<Value>) {
    provider
        .reply(
            "v1/playlists/detail",
            json!({
                "id": PLAYLIST, "name": "Owned aborted host selection", "snapshot": version,
                "total_count": items.len(), "attribution": {"label": "Owned aborted host provider"}
            }),
        )
        .await;
    provider
        .reply(
            "v1/playlists/items",
            json!({
                "playlist_id": PLAYLIST, "snapshot": version, "items": items, "next_cursor": null
            }),
        )
        .await;
}

struct Host {
    address: SocketAddr,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl Host {
    async fn start(services: Services) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let config = hito_server::Config {
            origin: format!("http://{address}"),
            ..Default::default()
        };
        let app = hito_server::router_with_config(services.clone(), config);
        let (stop, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            hito_server::serve(listener, app, services, async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
        });
        Self {
            address,
            stop: Some(stop),
            task: Some(task),
        }
    }

    async fn abort_public_future(&mut self) {
        let task = self.task.as_mut().unwrap();
        task.abort();
        let result = tokio::time::timeout(Duration::from_secs(2), task)
            .await
            .expect("the public host future must acknowledge cancellation promptly");
        self.task.take();
        assert!(result.unwrap_err().is_cancelled());
    }

    fn request_shutdown(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }

    async fn finish(&mut self) -> bool {
        if let Some(task) = self.task.as_mut() {
            let result = tokio::time::timeout(Duration::from_secs(4), task).await;
            let Ok(result) = result else {
                return false;
            };
            self.task.take();
            if let Err(error) = result {
                assert!(error.is_cancelled());
            }
        }
        tokio::time::timeout(Duration::from_secs(4), async {
            while tokio::net::TcpStream::connect(self.address).await.is_ok() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .is_ok()
    }
}

struct ReleaseGate(Arc<provider::Gate>);

impl Drop for ReleaseGate {
    fn drop(&mut self) {
        self.0.open();
    }
}
