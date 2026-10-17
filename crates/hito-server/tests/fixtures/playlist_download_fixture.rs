use super::{host, http, provider, support};
use axum::http::{HeaderMap, StatusCode};
use hito_core::Services;
use serde_json::{Value, json};
use std::{net::SocketAddr, time::Duration};
use uuid::Uuid;

pub const HOST: &str = "playlist-download-host";
const RECORDING: &str = "recording/owned-A";
const PLAYLIST: &str = "playlist/owned-download";

pub struct Fixture {
    pub directory: tempfile::TempDir,
    pub provider: provider::ProviderServer,
    pub services: Services,
    pub audio: Vec<u8>,
    pub original: Vec<u8>,
}

pub struct Selection {
    pub token: String,
    pub recording: String,
    pub playlist: String,
    pub personal_playlist: Uuid,
    pub provider_source: String,
}

impl Fixture {
    pub async fn start() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let music = directory.path().join("music");
        std::fs::create_dir(&music).unwrap();
        let original = support::write_wav(&music.join("original.wav"), 23);
        let audio = support::write_wav(&directory.path().join("provider.wav"), 10);
        let provider = provider::ProviderServer::start().await;
        provider
            .reply(
                "v1/manifest",
                json!({
                    "id": "org.hito.playlist-download-fixture", "name": "Owned playlist audio",
                    "protocol_version": 1, "capabilities": ["sources", "playlists"]
                }),
            )
            .await;
        provider.reply("v1/recordings/detail", recording()).await;
        provider
            .reply(
                "v1/sources",
                json!({"items": [{
                    "id": "audio/owned-A", "label": "Original", "ready": true
                }]}),
            )
            .await;
        provider
            .audio(
                StatusCode::OK,
                HeaderMap::from_iter([
                    (
                        "content-type".parse().unwrap(),
                        "audio/wav".parse().unwrap(),
                    ),
                    (
                        "content-length".parse().unwrap(),
                        audio.len().to_string().parse().unwrap(),
                    ),
                ]),
                audio.clone(),
            )
            .await;
        let services = host::open(directory.path(), HOST).await;
        Self {
            directory,
            provider,
            services,
            audio,
            original,
        }
    }

    pub async fn prepare(&self, address: SocketAddr) -> Selection {
        let response = http::call(
            address,
            "POST",
            "/api/v1/setup",
            Some(json!({
                "username": "owner", "password": "owned playlist download password"
            })),
            None,
        )
        .await;
        assert_eq!(response.status, StatusCode::CREATED);
        let token = response.json()["token"].as_str().unwrap().to_owned();
        let service = post(
            address,
            &token,
            "/api/v1/plugins/services",
            json!({
                "name": "Owned playlist audio", "base_url": self.provider.url
            }),
            StatusCode::CREATED,
        )
        .await;
        let installation = post(
            address,
            &token,
            "/api/v1/plugins",
            json!({
                "service_id": service["id"], "service_revision": service["revision"]
            }),
            StatusCode::CREATED,
        )
        .await;
        let plugin = format!("/api/v1/plugins/{}", installation["id"].as_str().unwrap());
        let adopted = post(address, &token, &format!("{plugin}/recordings"), json!({
            "installation_revision": installation["revision"], "provider_recording_id": RECORDING
        }), StatusCode::OK).await;
        let recording = adopted["id"].as_str().unwrap().to_owned();
        Uuid::parse_str(&recording).unwrap();
        let sources = http::get(
            address,
            &token,
            &format!("/api/v1/recordings/{recording}/sources"),
        )
        .await;
        assert_eq!(sources.as_array().unwrap().len(), 1);
        assert!(sources[0]["ready"] == true);
        assert!(sources[0]["provider"] == "plugin");
        let provider_source = sources[0]["id"].as_str().unwrap().to_owned();
        self.snapshot("two-occurrences", &["first", "duplicate"])
            .await;
        let followed = post(address, &token, &format!("{plugin}/playlists/follow"), json!({
            "request_id": Uuid::new_v4(), "installation_revision": installation["revision"],
            "playlist_reference": "https://provider.invalid/owned-playlist-download", "interval": "hourly"
        }), StatusCode::OK).await;
        let playlist = followed["playlist_id"].as_str().unwrap().to_owned();
        Uuid::parse_str(&playlist).unwrap();
        let personal_playlist = Uuid::new_v4();
        let actions = [
            json!({"kind": "playlist_create", "playlist_id": personal_playlist, "name": "My independent playlist"}),
            json!({"kind": "entry_insert", "playlist_id": personal_playlist, "entry_id": Uuid::new_v4(), "recording_id": recording, "position": 0}),
            json!({"kind": "recording_title_set", "recording_id": recording, "title": "My retained title"}),
            json!({"kind": "recording_artist_set", "recording_id": recording, "artist": "My retained artist"}),
            json!({"kind": "like_set", "recording_id": recording, "liked": true}),
            json!({"kind": "progress_set", "recording_id": recording, "position_ms": 700, "speed": 1.25}),
        ];
        let operations: Vec<Value> = actions
            .into_iter()
            .map(|action| json!({"id": Uuid::new_v4(), "action": action}))
            .collect();
        let receipt = post(
            address,
            &token,
            "/api/v1/sync",
            json!({
                "device_id": "playlist-download-client", "operations": operations
            }),
            StatusCode::OK,
        )
        .await;
        assert_eq!(receipt["acknowledged"].as_array().unwrap().len(), 6);
        Selection {
            token,
            recording,
            playlist,
            personal_playlist,
            provider_source,
        }
    }

    async fn snapshot(&self, snapshot: &str, occurrences: &[&str]) {
        self.provider.reply("v1/playlists/detail", json!({
            "id": PLAYLIST, "name": "Owned selection", "snapshot": snapshot,
            "total_count": occurrences.len(), "attribution": {"label": "Owned audio fixture"}
        })).await;
        let items: Vec<Value> = occurrences.iter().map(|occurrence| json!({
            "occurrence_id": occurrence, "item": {"kind": "recording", "recording": recording()}
        })).collect();
        self.provider.reply("v1/playlists/items", json!({
            "playlist_id": PLAYLIST, "snapshot": snapshot, "items": items, "next_cursor": null
        })).await;
    }

    pub async fn refresh(
        &self,
        address: SocketAddr,
        selection: &Selection,
        snapshot: &str,
        occurrences: &[&str],
    ) {
        self.snapshot(snapshot, occurrences).await;
        let path = format!("/api/v1/playlists/{}/follow", selection.playlist);
        let request_id = Uuid::new_v4();
        let receipt = post(
            address,
            &selection.token,
            &format!("{path}/refresh"),
            json!({"request_id": request_id}),
            StatusCode::ACCEPTED,
        )
        .await;
        assert!(receipt["request_id"] == request_id.to_string());
        assert!(receipt["playlist_id"] == selection.playlist);
        Uuid::parse_str(receipt["job_id"].as_str().unwrap()).unwrap();
        wait_for_json(address, &selection.token, &path, |state| {
            assert!(matches!(
                state["status"].as_str(),
                Some("queued" | "running" | "ready")
            ));
            state["status"] == "ready" && state["snapshot"] == snapshot
        })
        .await;
    }

    pub async fn saved_state(&self, address: SocketAddr, selection: &Selection) -> Value {
        let token = &selection.token;
        let mut catalog = http::get(address, token, "/api/v1/recordings?limit=100").await;
        let mut playlist = http::get(
            address,
            token,
            &format!("/api/v1/playlists/{}", selection.personal_playlist),
        )
        .await;
        let mut activity = http::get(address, token, "/api/v1/activity").await;
        for rows in [
            &mut catalog["items"],
            &mut playlist["entries"],
            &mut activity["history"],
        ] {
            for row in rows.as_array_mut().unwrap() {
                row.as_object_mut().unwrap().remove("available");
            }
        }
        let metadata = http::get(
            address,
            token,
            &format!("/api/v1/recordings/{}/metadata", selection.recording),
        )
        .await;
        json!({"catalog": catalog, "playlist": playlist, "activity": activity, "metadata": {
            "recording_id": metadata["recording_id"], "title": metadata["title"], "artist": metadata["artist"]
        }})
    }
}

pub async fn wait_for_json(
    address: SocketAddr,
    token: &str,
    path: &str,
    ready: impl Fn(&Value) -> bool,
) -> Value {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let state = http::get(address, token, path).await;
            if ready(&state) {
                return state;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("the joined host must reach the requested public state")
}

async fn post(
    address: SocketAddr,
    token: &str,
    path: &str,
    body: Value,
    expected: StatusCode,
) -> Value {
    let response = http::call(address, "POST", path, Some(body), Some(token)).await;
    assert_eq!(response.status, expected);
    response.json()
}

fn recording() -> Value {
    json!({"id": RECORDING, "title": "Owned recording A", "artist": "Owned artist", "duration_ms": 1000})
}
