use super::{fixture, http, support};
use axum::http::{HeaderMap, StatusCode};
use serde_json::{Value, json};
use std::net::SocketAddr;
use uuid::Uuid;

const PLAYLIST: &str = "playlist/owned-download";
pub const SOURCE_A: &str = "audio/owned-A";
pub const SOURCE_B: &str = "audio/owned-B";

#[derive(Clone, Copy)]
pub enum Item {
    First,
    Duplicate,
    Unmatched,
    Added,
}

impl Item {
    fn value(self) -> Value {
        let (occurrence, suffix) = match self {
            Self::First => ("first", "A"),
            Self::Duplicate => ("duplicate", "A"),
            Self::Unmatched => ("unmatched", "U"),
            Self::Added => ("added", "B"),
        };
        json!({
            "occurrence_id": occurrence,
            "item": {"kind": "recording", "recording": recording(suffix)}
        })
    }
}

pub struct FreshFollow {
    pub selection: fixture::Selection,
    pub b_recording: String,
    pub b_provider_source: String,
    pub unmatched_recording: String,
    pub a_entries: [String; 2],
    pub unmatched_entry: String,
    pub b_audio: Vec<u8>,
}

impl FreshFollow {
    pub async fn prepare(fixture: &fixture::Fixture, address: SocketAddr) -> Self {
        let selection = fixture.prepare(address).await;
        let token = &selection.token;
        let detail_path = format!("/api/v1/playlists/{}", selection.playlist);
        let original = http::get(address, token, &detail_path).await;
        assert_eq!(original["entries"].as_array().unwrap().len(), 2);
        let a_entries = [
            occurrence(&original, "first")["entry_id"]
                .as_str()
                .unwrap()
                .to_owned(),
            occurrence(&original, "duplicate")["entry_id"]
                .as_str()
                .unwrap()
                .to_owned(),
        ];
        assert_ne!(a_entries[0], a_entries[1]);
        for id in &a_entries {
            Uuid::parse_str(id).unwrap();
        }
        let settings = http::get(address, token, "/api/v1/plugins").await;
        let installations = settings["installations"].as_array().unwrap();
        assert_eq!(installations.len(), 1);
        let installation = &installations[0];
        fixture
            .provider
            .reply("v1/recordings/detail", recording("B"))
            .await;
        fixture
            .provider
            .reply(
                "v1/sources",
                json!({"items": [{"id": SOURCE_B, "label": "Original", "ready": true}]}),
            )
            .await;
        let response = http::call(
            address,
            "POST",
            &format!(
                "/api/v1/plugins/{}/recordings",
                installation["id"].as_str().unwrap()
            ),
            Some(json!({
                "installation_revision": installation["revision"],
                "provider_recording_id": "recording/owned-B"
            })),
            Some(token),
        )
        .await;
        assert_eq!(response.status, StatusCode::OK);
        let b_recording = response.json()["id"].as_str().unwrap().to_owned();
        Uuid::parse_str(&b_recording).unwrap();
        assert_ne!(b_recording, selection.recording);
        let b_audio = support::write_wav(&fixture.directory.path().join("provider-B.wav"), 37);
        assert!(b_audio != fixture.audio);
        let detail = refresh(
            fixture,
            address,
            &selection,
            "fresh-with-unmatched",
            &[Item::First, Item::Duplicate, Item::Unmatched],
        )
        .await;
        assert_eq!(detail["entries"].as_array().unwrap().len(), 3);
        assert!(occurrence(&detail, "first")["entry_id"] == a_entries[0]);
        assert!(occurrence(&detail, "duplicate")["entry_id"] == a_entries[1]);
        for name in ["first", "duplicate"] {
            assert!(occurrence(&detail, name)["recording_id"] == selection.recording);
        }
        let unmatched = occurrence(&detail, "unmatched");
        let unmatched_recording = unmatched["recording_id"].as_str().unwrap().to_owned();
        let unmatched_entry = unmatched["entry_id"].as_str().unwrap().to_owned();
        Uuid::parse_str(&unmatched_recording).unwrap();
        Uuid::parse_str(&unmatched_entry).unwrap();
        assert_ne!(unmatched_recording, selection.recording);
        assert_ne!(unmatched_recording, b_recording);
        let a_source = ready_source(address, token, &selection.recording).await;
        let b_provider_source = ready_source(address, token, &b_recording).await;
        assert_eq!(a_source, selection.provider_source);
        assert_ne!(a_source, b_provider_source);
        let sources = http::get(
            address,
            token,
            &format!("/api/v1/recordings/{unmatched_recording}/sources"),
        )
        .await;
        assert!(sources.as_array().unwrap().is_empty());
        Self {
            selection,
            b_recording,
            b_provider_source,
            unmatched_recording,
            a_entries,
            unmatched_entry,
            b_audio,
        }
    }

    pub async fn serve_b_audio(&self, fixture: &fixture::Fixture) {
        fixture
            .provider
            .audio(
                StatusCode::OK,
                HeaderMap::from_iter([
                    (
                        "content-type".parse().unwrap(),
                        "audio/wav".parse().unwrap(),
                    ),
                    (
                        "content-length".parse().unwrap(),
                        self.b_audio.len().to_string().parse().unwrap(),
                    ),
                ]),
                self.b_audio.clone(),
            )
            .await;
    }
}

pub async fn refresh(
    fixture: &fixture::Fixture,
    address: SocketAddr,
    selection: &fixture::Selection,
    snapshot: &str,
    items: &[Item],
) -> Value {
    fixture
        .provider
        .reply(
            "v1/playlists/detail",
            json!({
                "id": PLAYLIST, "name": "Owned selection", "snapshot": snapshot,
                "total_count": items.len(), "attribution": {"label": "Owned audio fixture"}
            }),
        )
        .await;
    let items: Vec<Value> = items.iter().map(|item| item.value()).collect();
    fixture
        .provider
        .reply(
            "v1/playlists/items",
            json!({
                "playlist_id": PLAYLIST, "snapshot": snapshot, "items": items, "next_cursor": null
            }),
        )
        .await;
    let path = format!("/api/v1/playlists/{}", selection.playlist);
    let request_id = Uuid::new_v4();
    let response = http::call(
        address,
        "POST",
        &format!("{path}/follow/refresh"),
        Some(json!({"request_id": request_id})),
        Some(&selection.token),
    )
    .await;
    assert_eq!(response.status, StatusCode::ACCEPTED);
    let receipt = response.json();
    assert!(receipt["request_id"] == request_id.to_string());
    assert!(receipt["playlist_id"] == selection.playlist);
    Uuid::parse_str(receipt["job_id"].as_str().unwrap()).unwrap();
    fixture::wait_for_json(
        address,
        &selection.token,
        &format!("{path}/follow"),
        |state| {
            assert!(matches!(
                state["status"].as_str(),
                Some("queued" | "running" | "ready")
            ));
            state["status"] == "ready" && state["snapshot"] == snapshot
        },
    )
    .await;
    http::get(address, &selection.token, &path).await
}

pub fn occurrence<'a>(detail: &'a Value, name: &str) -> &'a Value {
    detail["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["origin"]["occurrence_id"] == name)
        .expect("the accepted Follow must preserve the provider occurrence")
}

async fn ready_source(address: SocketAddr, token: &str, recording: &str) -> String {
    let sources = http::get(
        address,
        token,
        &format!("/api/v1/recordings/{recording}/sources"),
    )
    .await;
    assert_eq!(sources.as_array().unwrap().len(), 1);
    assert!(sources[0]["provider"] == "plugin");
    assert!(sources[0]["ready"] == true);
    let id = sources[0]["id"].as_str().unwrap().to_owned();
    Uuid::parse_str(&id).unwrap();
    id
}

fn recording(suffix: &str) -> Value {
    json!({
        "id": format!("recording/owned-{suffix}"),
        "title": format!("Owned recording {suffix}"), "artist": "Owned artist", "duration_ms": 1000
    })
}
